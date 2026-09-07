#!/usr/bin/env python3
"""Detectors for the bug classes that have actually shipped in this repo.

Each detector corresponds to a row in the Bug Class Registry in
`.specify/memory/constitution.md`. The rule the registry states: a bug fixed after a
release leaves behind a detector here, so the next instance of the same class fails a
gate instead of reaching a user.

## Why there is a baseline

The first run of these detectors found 1444 instances. A gate that fails 1444 times is a
gate everyone learns to bypass, which is worse than no gate — so the gate enforces *no new
instances* instead of *zero instances*. `antipatterns-baseline.txt` lists the findings that
existed when each detector was written. A finding absent from the baseline fails the gate;
a baseline entry that no longer fires also fails it, which is what keeps the baseline
shrinking instead of rotting. Adding a line to the baseline is a tracked edit that shows up
in review, so silencing a finding is a visible choice rather than a quiet one.

## Why Python and not shell

The shell version nested `python3 - <<'PY'` inside `$( )`. macOS ships bash 3.2, which
mis-parses that when the heredoc body contains a single quote — the whole script failed to
parse, so every check "passed". A gate that cannot run is not a gate that passed, which is
the same fault as the vacuous tests these detectors look for.

Usage:
    scripts/gates/antipatterns.py                     # every detector, against the baseline
    scripts/gates/antipatterns.py empty-tests         # one detector
    scripts/gates/antipatterns.py --all-findings      # ignore the baseline, print everything
    scripts/gates/antipatterns.py --write-baseline    # record current findings as the baseline

Exit: 0 = no new findings, 2 = at least one new finding (or a stale baseline entry).
"""

from __future__ import annotations

import pathlib
import re
import sys
from dataclasses import dataclass

ROOT = pathlib.Path(__file__).resolve().parents[2]
BASELINE = ROOT / "scripts/gates/antipatterns-baseline.txt"


@dataclass(frozen=True)
class Finding:
    check: str
    location: str  # "path:line" or "path"
    message: str

    def key(self) -> str:
        return f"{self.check}\t{self.location}"


# ---------------------------------------------------------------------------
# File sets
# ---------------------------------------------------------------------------


def _rs_files(*globs: str) -> list[pathlib.Path]:
    out: set[pathlib.Path] = set()
    for g in globs:
        out.update(ROOT.glob(g))
    return sorted(p for p in out if p.is_file())


def src_files() -> list[pathlib.Path]:
    return _rs_files("crates/*/src/**/*.rs")


def test_files() -> list[pathlib.Path]:
    return _rs_files("crates/*/tests/**/*.rs")


def rel(p: pathlib.Path) -> str:
    return str(p.relative_to(ROOT))


def blank_inline_tests(text: str) -> str:
    """Replace every `#[cfg(test)] mod ... { ... }` body with blank lines.

    `src_files()` and `test_files()` split by path, so a `#[cfg(test)] mod tests` living
    inside a `src/` file is scanned by the src-only detectors and skipped by the test-only
    ones — both wrong. Detectors that ask "does the shipped code do X" call this first.
    Every offset is preserved (newlines kept, everything else blanked to a space), so
    reported line numbers still point at the real line and `body_after` spans computed
    against the original text stay valid.
    """
    chars = list(text)
    for m in re.finditer(r"#\[cfg\(test\)\]", text):
        span = body_after(text, m.end())
        if span is None:
            continue
        for k in range(*span):
            if chars[k] != "\n":
                chars[k] = " "
    return "".join(chars)


# ---------------------------------------------------------------------------
# A minimal Rust body scanner
#
# Regexes cannot find the end of a function, and every detector below needs to ask a
# question about one function's body. This walks braces while skipping the three things
# that make brace counting wrong: line comments, block comments, and string literals
# (including raw strings, which ObjectScript code blocks in this crate use heavily).
# ---------------------------------------------------------------------------


def _skip_string(text: str, i: int) -> int:
    """Index just past the string literal starting at `i` (which is `"` or `r#*"`)."""
    if text[i] == "r":
        j = i + 1
        hashes = 0
        while j < len(text) and text[j] == "#":
            hashes += 1
            j += 1
        if j >= len(text) or text[j] != '"':
            return i + 1
        close = '"' + "#" * hashes
        end = text.find(close, j + 1)
        return len(text) if end < 0 else end + len(close)
    j = i + 1
    while j < len(text):
        if text[j] == "\\":
            j += 2
            continue
        if text[j] == '"':
            return j + 1
        j += 1
    return len(text)


def body_after(text: str, start: int) -> tuple[int, int] | None:
    """Byte range of the `{...}` block at or after `start`, brace-matched."""
    i = text.find("{", start)
    if i < 0:
        return None
    depth = 0
    j = i
    while j < len(text):
        c = text[j]
        if c == "/" and text[j : j + 2] == "//":
            j = text.find("\n", j)
            if j < 0:
                return None
            continue
        if c == "/" and text[j : j + 2] == "/*":
            j = text.find("*/", j)
            if j < 0:
                return None
            j += 2
            continue
        if c == '"' or (c == "r" and re.match(r'r#*"', text[j : j + 8])):
            j = _skip_string(text, j)
            continue
        if c == "'" and re.match(r"'(\\.|[^\\'])'", text[j : j + 4]):
            j += len(re.match(r"'(\\.|[^\\'])'", text[j : j + 4]).group(0))
            continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return (i, j + 1)
        j += 1
    return None


TEST_ATTR = re.compile(r"#\[(?:tokio::)?test\]")
FN_DECL = re.compile(r"\b(?:async\s+)?fn\s+([A-Za-z0-9_]+)")


def test_fns(text: str):
    """Yield (name, line, body, attrs) for every #[test] / #[tokio::test] function.

    `attrs` is everything between the test attribute and the `fn` keyword, which is where
    `#[should_panic]` and `#[ignore]` sit.
    """
    for attr in TEST_ATTR.finditer(text):
        decl = FN_DECL.search(text, attr.end())
        if not decl or decl.start() - attr.end() > 200:
            continue
        span = body_after(text, decl.end())
        if not span:
            continue
        yield (
            decl.group(1),
            text.count("\n", 0, decl.start()) + 1,
            text[span[0] : span[1]],
            text[attr.start() : decl.start()],
        )


def strip_comments(code: str) -> str:
    out = []
    i = 0
    while i < len(code):
        if code[i : i + 2] == "//":
            nl = code.find("\n", i)
            i = len(code) if nl < 0 else nl
            continue
        if code[i : i + 2] == "/*":
            end = code.find("*/", i)
            i = len(code) if end < 0 else end + 2
            continue
        out.append(code[i])
        i += 1
    return "".join(out)


def mask_comments(code: str) -> str:
    """Blank out comment text, keeping every byte offset and newline where it was.

    `strip_comments` is for brace-scanning bodies, where only the code matters. Any detector
    that reports a *line number* needs offsets preserved, and any detector whose pattern
    could appear in prose needs comments gone: the doc comments explaining these very bugs
    quote the bad code as an example, and a scanner that reads them flags the fix as the
    defect. That is the same "two artifacts agreeing" failure the gates exist to catch, so
    it is worth the extra pass.
    """
    out = list(code)
    i = 0
    while i < len(code):
        if code[i : i + 2] == "//":
            nl = code.find("\n", i)
            end = len(code) if nl < 0 else nl
        elif code[i : i + 2] == "/*":
            close = code.find("*/", i)
            end = len(code) if close < 0 else close + 2
        else:
            i += 1
            continue
        for j in range(i, end):
            if out[j] != "\n":
                out[j] = " "
        i = end
    return "".join(out)


# ---------------------------------------------------------------------------
# empty-tests
#
# Shipped instance: four tests in gate_macro.rs had a doc comment describing what
# tool_gate! does and no code at all. They reported ok on every run and showed up in the
# count as four tests covering the policy gate. Four green lines beside a security gate is
# worse than no lines.
# ---------------------------------------------------------------------------

# `testing.rs` defines the IAD_BINARY resolver and `test_testing_helpers.rs` is the only
# thing that tests it, so both have to touch the raw variable and the relative path the
# resolver exists to fix. Every other file must go through the resolver.
RESOLVER_FILES = {"testing.rs", "test_testing_helpers.rs"}

ASSERTS = re.compile(
    r"\bassert\w*!|\bpanic!\b|\bunreachable!\b|\.unwrap\(|\.expect\(|\?;|\bmatches!\b"
)


ASSERT_HELPER_DECL = re.compile(
    r"\bfn[ \t]+(assert_[A-Za-z0-9_]+)[ \t]*(?:<[^>(]*>)?[ \t]*\("
)
ASSERT_HELPER_CALL = re.compile(r"\b(assert_[A-Za-z0-9_]+)[ \t]*\(")


def asserting_helpers(texts) -> set[str]:
    """Names of `assert_*` functions whose own body asserts.

    A test reading `assert_advertised_contracts(BATCH1)` does assert — the assertion is one
    call away, in a helper fourteen tests share, and inlining it fourteen times to satisfy a
    regex would be the gate driving the code rather than the other way round. Naming alone
    is not enough to trust, though: `fn assert_nothing() {}` would sail through. So a call
    counts as an assertion only when the helper it names carries one itself.
    """
    helpers: set[str] = set()
    for text in texts:
        for m in ASSERT_HELPER_DECL.finditer(text):
            span = body_after(text, m.end())
            if span and ASSERTS.search(strip_comments(text[span[0] : span[1]])):
                helpers.add(m.group(1))
    return helpers


def empty_tests_findings(files: dict[str, str]) -> list[Finding]:
    found = []
    helpers = asserting_helpers(files.values())
    for path, text in files.items():
        for name, line, body, attrs in test_fns(text):
            code = strip_comments(body)
            if ASSERTS.search(code):
                continue
            if any(c in helpers for c in ASSERT_HELPER_CALL.findall(code)):
                continue
            # `#[should_panic(expected = "...")]` puts the assertion in the attribute: the
            # test fails if the call returns, and fails if it panics with the wrong message.
            # A body with no assert! is the correct shape for one.
            if "should_panic" in attrs:
                continue
            # A test that names itself as trivial and says why is an honest marker, not a
            # lie. `gate_macro.rs` uses one so the next person finds the note.
            if "nothing_to_assert" in name or "deliberately_trivial" in name:
                continue
            found.append(
                Finding(
                    "empty-tests",
                    f"{path}:{line}",
                    f"`{name}` asserts nothing — no assert!, unwrap, expect, or `?`. It "
                    "reports ok without checking anything. Assert something, or delete it "
                    "and put the reason in a comment.",
                )
            )
    return found


def check_empty_tests() -> list[Finding]:
    return empty_tests_findings(
        {rel(p): p.read_text(errors="replace") for p in src_files() + test_files()}
    )


# ---------------------------------------------------------------------------
# vacuous-tests
#
# Shipped instance: five nopws_101 tests defaulted IAD_BINARY to the relative path
# ./target/debug/iris-agentic-dev. Cargo runs tests with the CWD at the crate root, so that
# path never resolved, the skip branch was always taken, and the tests reported ok for the
# whole 1.3.x line.
#
# Two faults, so two rules: a resource path that depends on the working directory, and a
# missing resource reported by returning instead of failing.
# ---------------------------------------------------------------------------

REL_TARGET_PATH = re.compile(r'"\.[/\\]target[/\\](debug|release)[/\\]')
SKIP_GUARD = re.compile(r"\.exists\(\)|IAD_BINARY|CARGO_BIN_EXE")
# `panic!` or `assert` *somewhere* in the body says nothing about the skip branch — the two
# tests beside the one this originally caught both asserted heavily and still returned early on
# a missing binary. Only the shared helper counts: it is the thing that makes a skip loud.
LOUD_SKIP = re.compile(r"IAD_ALLOW_SKIP|require_iad_binary|iad_binary_path")
# A test returns `()`, so a bare `return;` is always an early exit and never a result.
BARE_RETURN = re.compile(r"\breturn\s*;")


def check_vacuous_tests() -> list[Finding]:
    found = []
    for path in src_files() + test_files():
        raw = path.read_text(errors="replace")
        # `testing.rs` documents the bug by quoting the path it replaced; masked so the
        # explanation is not itself a finding.
        text = mask_comments(raw)
        for m in [] if path.name in RESOLVER_FILES else REL_TARGET_PATH.finditer(text):
            found.append(
                Finding(
                    "vacuous-tests",
                    f"{rel(path)}:{text.count(chr(10), 0, m.start()) + 1}",
                    "relative path to a build artifact: a test's working directory is the "
                    "crate root, not the workspace root, so this never resolves and the "
                    "test skips forever. Use `testing::require_iad_binary()`, which "
                    "resolves from CARGO_MANIFEST_DIR at compile time.",
                )
            )
        for name, line, body, _attrs in test_fns(text):
            code = strip_comments(body)
            # Only local-resource skips. A live-IRIS skip is the project's #[ignore]
            # convention and is a separate, documented decision.
            if not SKIP_GUARD.search(code):
                continue
            if not BARE_RETURN.search(code):
                continue
            if LOUD_SKIP.search(code):
                continue
            found.append(
                Finding(
                    "vacuous-tests",
                    f"{rel(path)}:{line}",
                    f"`{name}` returns without asserting when a local resource is "
                    "missing, so 'ran nothing' is indistinguishable from 'verified "
                    "everything' in the summary. Use `testing::require_iad_binary()`, "
                    "which panics unless IAD_ALLOW_SKIP is set.",
                )
            )
    return found


# ---------------------------------------------------------------------------
# mcp-subcommand / env-pinning
#
# Both ask a question about one spawn expression, so they share the extractor.
#
# Shipped instances: nopws_101 spawned the bare binary, which prints usage and exits, so
# every response was empty and the assertions read that as "field absent". And the
# gate-refusal tests in admin_e2e inherited IRIS_WRITE_TOOLS_ENABLED from the CI e2e job,
# which sets it at job level — so the refusals were authorized live calls that passed for
# the wrong reason.
# ---------------------------------------------------------------------------

SPAWN_START = re.compile(r"Command::new\(\s*([^)]*)\)")
SPAWN_END = re.compile(r"\.(spawn|output|status)\(\)")


def spawn_exprs(text: str):
    """Yield (line, expression-text) for each Command::new(...) ... .spawn() chain."""
    for m in SPAWN_START.finditer(text):
        arg = m.group(1)
        if not re.search(r"bin|binary|BINARY|exe|iad", arg):
            continue
        end = SPAWN_END.search(text, m.end())
        if not end:
            continue
        # A chain that runs past the next Command::new is not one chain.
        nxt = SPAWN_START.search(text, m.end())
        if nxt and nxt.start() < end.start():
            continue
        yield text.count("\n", 0, m.start()) + 1, text[m.start() : end.end()]


def check_mcp_subcommand() -> list[Finding]:
    found = []
    for path in test_files():
        text = path.read_text(errors="replace")
        for line, expr in spawn_exprs(text):
            if re.search(r'\.arg\("(mcp|tool|check|--)', expr) or ".args(" in expr:
                continue
            if "clean_mcp_command" in expr:
                continue
            found.append(
                Finding(
                    "mcp-subcommand",
                    f"{rel(path)}:{line}",
                    "spawns the binary with no subcommand. The MCP server is "
                    "`iris-agentic-dev mcp`; a bare spawn prints the usage banner and "
                    "exits 2, which a test reading stdout for JSON-RPC sees as empty "
                    "output. Use `testing::clean_mcp_command`.",
                )
            )
    return found


def check_env_pinning() -> list[Finding]:
    found = []
    for path in test_files():
        text = path.read_text(errors="replace")
        for line, expr in spawn_exprs(text):
            if "clean_command" in expr or "clean_mcp_command" in expr:
                continue
            missing = [
                v
                for v in ("IRIS_WRITE_TOOLS_ENABLED", "IRIS_DESTRUCTIVE_TOOLS_ENABLED")
                if v not in expr
            ]
            if not missing:
                continue
            found.append(
                Finding(
                    "env-pinning",
                    f"{rel(path)}:{line}",
                    f"spawn does not pin {', '.join(missing)}. The CI e2e job sets both "
                    "at job level, so the same test means different things in the `test` "
                    "job and the `e2e-tests` job. Set or `env_remove` them, or go through "
                    "`testing::clean_command`.",
                )
            )
    return found


# ---------------------------------------------------------------------------
# error-sentinels
#
# Shipped instance: fourteen sites hand-rolled starts_with("ERROR: "), which does not match
# the ERROR($ZERROR): or ERROR($DEVICE): shapes, so IRIS-side failures were returned to the
# caller as success — an empty global preview with a valid kill token, a skill body that was
# an error message, a delete that reported "forgotten" without killing anything.
# ---------------------------------------------------------------------------

# Only the *bare* sentinel. `strip_prefix("ERROR:NAMESPACE_EXISTS:")` is a tool-generated
# code with a defined meaning and is fine; `starts_with("ERROR: ")` is a claim to recognise
# IRIS failure in general, which is the claim that was wrong at fourteen sites.
HAND_ROLLED = re.compile(r'(starts_with|contains|strip_prefix)\(\s*"ERROR: ?"')


def check_error_sentinels() -> list[Finding]:
    found = []
    for path in src_files():
        if path.name == "connection.rs" or path.name == "global.rs":
            continue  # is_generator_error lives in one; the other strips after calling it
        text = mask_comments(path.read_text(errors="replace"))
        for m in HAND_ROLLED.finditer(text):
            found.append(
                Finding(
                    "error-sentinels",
                    f"{rel(path)}:{text.count(chr(10), 0, m.start()) + 1}",
                    "hand-rolled IRIS failure check. Call "
                    "`iris::connection::is_generator_error` — it knows all four shapes, "
                    "including ERROR($ZERROR): and ERROR($DEVICE): which a "
                    'starts_with("ERROR: ") misses. One definition means a fifth shape is '
                    "one edit, not fourteen.",
                )
            )
    return found


# ---------------------------------------------------------------------------
# device-capture
#
# Shipped instance: run^SystemPerformance and waittime^SystemPerformance switch the current
# device. execute_via_generator captures output by pointing $IO at a temp file, so the Write
# after the call landed on SystemPerformance's device and the tool returned an empty result
# plus a residual $ZERROR while the run had actually started.
# ---------------------------------------------------------------------------

DEVICE_ROUTINES = re.compile(
    r"\^(SystemPerformance|JOURNAL|BACKUP|DATABASE|SECURITY|GBLOCKCOPY|%GSIZE|DBSIZE"
    r"|INTEGRIT|RESTORE|%FREECNT)\b"
)
DEVICE_CLASSES = re.compile(
    r"##class\((Config\.\w+|SYS\.Mirror|%SYSTEM\.OBJ)\)"
    # Ens.Director is method-specific. The device moves when a method starts, stops or
    # re-jobs the production's worker jobs; `GetProductionStatus`, `ProductionNeedsUpdate`
    # and `SetAutoStart` only read state or write a config global. Matching the bare class
    # flagged 8 read-only call sites for every 2 real ones, and a detector that is mostly
    # wrong is a detector people learn to skip.
    r"|##class\(Ens\.Director\)\.(Start|Stop|Update|Recover|Clean)Production"
    r"|##class\(Ens\.Director\)\.(Start|Stop|TempStop)Item"
)
IO_SNAPSHOT = re.compile(r"Set t?io=\$IO", re.I)


def device_capture_findings(files: dict[str, str]) -> list[Finding]:
    found = []
    for path, raw in files.items():
        # A gate's whole job is to name the dangerous APIs. `src/policy/` holds blocklist
        # token constants and the doc comments explaining them — nothing there is ever handed
        # to IRIS, so a hit is guaranteed noise. Noise is what teaches people to bypass a
        # gate, which is the failure this suite exists to prevent.
        if "/src/policy/" in path:
            continue
        # `mask_comments` for the same reason `/src/policy/` is skipped: a doc comment naming
        # `waittime^SystemPerformance` to explain what the routine answers is documentation,
        # not a call, and nothing in a comment is ever handed to IRIS. Offsets survive the
        # mask, so the reported line still points at the real line.
        text = mask_comments(blank_inline_tests(raw))
        lines = text.splitlines()
        # $IO discipline is per-ObjectScript-block, and blocks here are raw string
        # literals. Approximate a block by a 40-line window, which is longer than any
        # generated block in the crate.
        for i, line in enumerate(lines, 1):
            hit = DEVICE_ROUTINES.search(line) or DEVICE_CLASSES.search(line)
            if not hit:
                continue
            window = "\n".join(lines[max(0, i - 21) : i + 20])
            if IO_SNAPSHOT.search(window):
                continue
            found.append(
                Finding(
                    "device-capture",
                    f"{path}:{i}",
                    f"calls `{hit.group(0)}`, which can switch the current device, with no "
                    "$IO snapshot within 20 lines. Wrap it: `Set tIO=$IO` / call / "
                    "`Use tIO`, then Write. Otherwise the output lands on the callee's "
                    "device and the tool sees an empty success.",
                )
            )
    return found


def check_device_capture() -> list[Finding]:
    return device_capture_findings(_src_texts())


# ---------------------------------------------------------------------------
# self-referential-gates
#
# Shipped instance: BULK_PHI_TOOLS named `view_message_body` from 051 until 1.3.2. No tool
# by that name was ever registered, so check_bulk_phi_gate never matched and PHI message
# bodies came back with no policy check at all. Two tests covered it and both passed,
# because both asserted the same wrong string the constant held. A gate compared against
# itself agrees with itself.
# ---------------------------------------------------------------------------

# A gate list is a *list of tool names*: `&[&str]`. Requiring the type keeps error-code
# string constants (`pub const ERR_POLICY_GATE: &str = "POLICY_GATE"`) out — those name a
# code, not a tool, so there is no registry to check them against.
GATE_LIST = re.compile(
    r"pub const ([A-Z][A-Z0-9_]*(?:TOOLS|GATES?|BLOCKED))\s*:\s*&\[&str\]"
)

# What counts as the router's own registry. `CLASSIFICATION` is the gate table;
# `registered_tool_names` walks the live `IrisTools` instance, which is stronger still.
# Either one is an independent source; a literal in the test is not.
REGISTRY_TOKENS = ("CLASSIFICATION", "registered_tool_names")


def check_self_referential_gates() -> list[Finding]:
    tests_text = {p: p.read_text(errors="replace") for p in test_files()}
    found = []
    for path in src_files():
        text = path.read_text(errors="replace")
        for m in GATE_LIST.finditer(text):
            name = m.group(1)
            line = text.count("\n", 0, m.start()) + 1
            naming = [t for t, body in tests_text.items() if name in body]
            if not naming:
                found.append(
                    Finding(
                        "self-referential-gates",
                        f"{rel(path)}:{line}",
                        f"`{name}` is a gate list with no test in crates/*/tests. A typo'd "
                        "tool name in a gate list fails open and looks exactly like a "
                        "permitted call.",
                    )
                )
                continue
            if not any(
                any(tok in tests_text[t] for tok in REGISTRY_TOKENS) for t in naming
            ):
                found.append(
                    Finding(
                        "self-referential-gates",
                        f"{rel(path)}:{line}",
                        f"`{name}` is tested, but no test that names it also names one of "
                        f"{', '.join(REGISTRY_TOKENS)}. Walk every entry against the "
                        "router's own registry; asserting the constant against a literal "
                        "in the test proves only that the two agree.",
                    )
                )
    return found


# ---------------------------------------------------------------------------
# version-consistency / tool-name-refs — thin wrappers over their own scripts
# ---------------------------------------------------------------------------


def check_version_consistency() -> list[Finding]:
    import subprocess

    proc = subprocess.run(
        [sys.executable, str(ROOT / "scripts/gates/check_versions.py")],
        capture_output=True,
        text=True,
    )
    if proc.returncode not in (0, 1):
        return [
            Finding(
                "version-consistency",
                "scripts/gates/check_versions.py",
                f"the version extractor itself failed: {proc.stderr.strip()}. A check that "
                "cannot run is not a check that passed.",
            )
        ]
    out = []
    for row in proc.stdout.splitlines():
        parts = row.split("\t")
        if len(parts) != 4:
            continue
        path, key, got, want = parts
        out.append(
            Finding(
                "version-consistency",
                path,
                f"{key} is {got} but the workspace version is {want}. Every file that "
                "names the release version needs a cross-file assertion, per the release "
                "checklist.",
            )
        )
    return out


# ---------------------------------------------------------------------------
# binary-path
#
# Shipped instance: six test files in iris-agentic-dev-bin resolved `IAD_BINARY` with a bare
# `std::env::var("IAD_BINARY")` and handed the result straight to `Command::new`. Every doc
# comment in the repo says to pass `./target/debug/iris-agentic-dev`, and a workspace member's
# test binary runs with the *member* directory as its working directory — so the relative path
# never resolved and binary_098_server_probe failed all four of its tests with "binary not
# found" against a binary that was sitting right there.
#
# `core::testing::iad_binary_path` resolves relative values against the workspace root. The
# rule belongs in one place; six copies is six chances to get it wrong.
# ---------------------------------------------------------------------------

RAW_IAD_BINARY = re.compile(r'env::var\(\s*"IAD_BINARY"')

# The other half of the same bug: a hard-coded `"./target/..."` in Rust resolves against
# whichever directory the process happens to start in. The spec 112 accept block used to
# grep for this itself, which made two implementations of one rule — the defect
# `self-referential-gates` is about — and the grep had no exemption for the resolver's own
# tests, whose whole job is to feed it a relative path.
RELATIVE_TARGET = re.compile(r'"\./target/')


def check_binary_path() -> list[Finding]:
    found = []
    for path in test_files() + src_files():
        if path.name in RESOLVER_FILES:
            continue
        text = mask_comments(path.read_text(errors="replace"))
        for m in RAW_IAD_BINARY.finditer(text):
            found.append(
                Finding(
                    "binary-path",
                    f"{rel(path)}:{text.count(chr(10), 0, m.start()) + 1}",
                    "reads IAD_BINARY directly. Call "
                    "`iris_agentic_dev_core::testing::iad_binary_path()` (or "
                    "`require_iad_binary()`) — a relative IAD_BINARY, which is the form every "
                    "doc comment in this repo tells you to pass, resolves against the crate "
                    "directory here and not the workspace root.",
                )
            )
        for m in RELATIVE_TARGET.finditer(text):
            found.append(
                Finding(
                    "binary-path",
                    f"{rel(path)}:{text.count(chr(10), 0, m.start()) + 1}",
                    "hard-codes a relative path to a build artifact. A test binary runs with "
                    "the crate directory as its working directory, so `./target/...` does not "
                    "resolve — go through "
                    "`iris_agentic_dev_core::testing::iad_binary_path()` instead.",
                )
            )
    return found


# ---------------------------------------------------------------------------
# empty-config-value
#
# Shipped instance: `.cargo/config.toml` set `build.rustc-wrapper = ""` to keep the global
# sccache setting out of this repo. Cargo reads an empty wrapper as "no wrapper", so `cargo
# build` and `cargo test` were fine — but cargo-llvm-cov reads the key itself, does not apply
# that special case, and built the command `" " + rustc`. Every invocation died with
# `could not execute process ` /Users/…/bin/rustc --print cfg` (never executed)`, so the entire
# coverage gate was unrunnable from a720d2f onward with nothing to say so.
#
# An empty string is not how you turn a tool off. It is a value, and the next tool to read the
# key gets to decide what it means. Name a real passthrough (`/usr/bin/env`) or delete the key.
# ---------------------------------------------------------------------------

EMPTY_TOML_VALUE = re.compile(r'^\s*([A-Za-z0-9_.-]+)\s*=\s*""\s*$', re.M)

# Keys whose empty value is the documented, intended value rather than an off switch.
EMPTY_VALUE_OK = {"prefix"}


def check_empty_config_value() -> list[Finding]:
    found = []
    for path in sorted(ROOT.glob(".cargo/config.toml")) + sorted(
        ROOT.glob("crates/*/.cargo/config.toml")
    ):
        text = path.read_text(errors="replace")
        for m in EMPTY_TOML_VALUE.finditer(text):
            key = m.group(1)
            if key in EMPTY_VALUE_OK:
                continue
            found.append(
                Finding(
                    "empty-config-value",
                    f"{rel(path)}:{text.count(chr(10), 0, m.start()) + 1}",
                    f"sets `{key}` to the empty string. Cargo reads that as unset, other "
                    'tools that read the same key do not — `rustc-wrapper = ""` broke every '
                    "cargo-llvm-cov run while cargo build kept passing. Name a real value or "
                    "remove the key.",
                )
            )
    return found


# ---------------------------------------------------------------------------
# stale-coverage-objects
#
# Shipped instance: neither `scripts/coverage.sh` nor `scripts/check-coverage-floors.sh` ran
# `cargo llvm-cov clean` first. llvm-cov reports on every object file under llvm-cov-target,
# including test binaries left behind by earlier builds. A leftover binary carries its own
# instrumented copy of the library, that copy never runs, and the same source file is counted
# twice — once covered, once dark.
#
# Measured 2026-09-04: `policy/data_policy_gate.rs` read 50.00% with four copies of the core
# crate in the report, 98.04% after a clean. Overall read 75.64% against a floor of 88 with nine
# files apparently below floor; after a clean it read 87.68% with four. A whole release was one
# decision away from having its floors lowered to match leftovers.
#
# A gate whose verdict depends on what is lying in the target directory is not a gate. Any
# script that generates a coverage report has to clean first.
# ---------------------------------------------------------------------------

COVERAGE_SCRIPT_GLOB = "scripts/*coverage*.sh"


def check_stale_coverage_objects() -> list[Finding]:
    found = []
    for path in sorted(ROOT.glob(COVERAGE_SCRIPT_GLOB)):
        text = path.read_text(errors="replace")
        # Only scripts that produce a report can be diluted by stale objects.
        if "llvm-cov" not in text:
            continue
        if "llvm-cov clean" in text:
            continue
        found.append(
            Finding(
                "stale-coverage-objects",
                rel(path),
                "runs cargo llvm-cov without `cargo llvm-cov clean --workspace` first. "
                "Stale test binaries under llvm-cov-target carry dark copies of the library "
                "and every file reads low — this is how the 1.3.2 gate reported 75.64% for a "
                "tree that measures 87.68%. Clean before measuring.",
            )
        )
    return found


def check_tool_name_refs() -> list[Finding]:
    import subprocess

    proc = subprocess.run(
        [sys.executable, str(ROOT / "scripts/gates/check_tool_names.py")],
        capture_output=True,
        text=True,
    )
    if proc.returncode not in (0, 1):
        return [
            Finding(
                "tool-name-refs",
                "scripts/gates/check_tool_names.py",
                f"the tool-name extractor itself failed: {proc.stderr.strip()}. A check "
                "that cannot run is not a check that passed.",
            )
        ]
    out = []
    for row in proc.stdout.splitlines():
        parts = row.split("\t")
        if len(parts) != 3:
            continue
        path, line, name = parts
        out.append(
            Finding(
                "tool-name-refs",
                f"{path}:{line}",
                f"names `{name}`, which is not in tools::write_gate::CLASSIFICATION. "
                "Agents read these strings and act on them, so a wrong name is a "
                "functional bug, not a typo.",
            )
        )
    return out


# ---------------------------------------------------------------------------
# undeclared-params
#
# Shipped instance: 31 of 81 tools took `Parameters<AnyParams>`, a newtype around
# `serde_json::Value`. schemars has nothing to reflect from that, so each of those tools
# advertised `inputSchema: {"type": "object"}` — no properties, no types, no enums — and every
# key a caller got wrong was silently dropped. `stream_inspect` documented a `max_chars`
# parameter that existed nowhere in `crates/*/src`: a caller asking for 10,000 characters
# received the entire stream, with no error, for five minor versions.
#
# Feature 113 converted all 31 to typed params structs. Three shapes reopen the hole:
#
#   1. An untyped referent — `Parameters<serde_json::Value>` is `AnyParams` under a new name.
#   2. A params struct without `#[serde(deny_unknown_fields)]` — it declares its parameters
#      and still accepts everything else, which documents the surface without constraining it.
#      That is exactly the state `max_chars` lived in.
#   3. A `#[serde(flatten)]` field typed `Value` or `Map` — this satisfies both rules above
#      and accepts every key again, so the first two alone are not a guard.
# ---------------------------------------------------------------------------

STRUCT_DECL = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?struct[ \t]+([A-Za-z0-9_]+)", re.M
)
FIELD_DECL = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?([a-z_][A-Za-z0-9_]*)[ \t]*:[ \t]*([^,\n]+)",
    re.M,
)
OPEN_REFERENT = re.compile(r"Parameters<\s*(?:serde_json::)?(Value|AnyParams)\s*>")
FLATTEN_INTO_ANYTHING = re.compile(r"\bValue\b|\bMap<")


def attr_run(text: str, decl_start: int) -> str:
    """The attributes and doc comments immediately above the item starting at `decl_start`.

    Walks lines backwards while they still look like part of one attribute block, so a
    `#[derive(...)]` split across lines and the doc comment above it both come back as one
    string. Stops at the first line of real code, which is what keeps the run belonging to
    this item rather than the previous one.
    """
    out: list[str] = []
    for ln in reversed(text[:decl_start].splitlines()):
        s = ln.strip()
        if not s or s.startswith(("#[", "//", "*", "/*", "]", ")", '"')):
            if not s and not out:
                continue
            if not s:
                break
            out.append(ln)
            continue
        break
    return "\n".join(reversed(out))


def struct_items(text: str):
    """Yield (name, decl_start, body) for every struct declaration with a `{...}` body."""
    for m in STRUCT_DECL.finditer(text):
        span = body_after(text, m.end())
        if span is None:
            continue
        # A struct declared `struct X(Y);` has no brace body of its own; `body_after` would
        # hand back the next unrelated block, so require the brace to follow immediately.
        if text[m.end() : span[0]].strip() not in ("", "{"):
            continue
        yield m.group(1), m.start(), text[span[0] : span[1]], span[0]


def params_referents(files: dict[str, str]) -> set[str]:
    """Every struct named in a `Parameters<…>` position, reduced to its last path segment.

    This is what makes the check about schemas rather than about naming: only a referent's
    schema is published to a client, so only a referent can accept a key nobody declared.
    `interop.rs` is full of internal `…Params` argument bundles built in Rust by a dispatcher
    that already validated its own parameters — those never see a JSON object from outside.
    """
    out: set[str] = set()
    for raw in files.values():
        for m in PARAMS_REFERENT.finditer(mask_comments(raw)):
            out.add(m.group(1).split("::")[-1])
    return out


def schema_bearing_params(files: dict[str, str]) -> set[str]:
    """The referents, plus the params types they hold as fields.

    A field typed `Vec<SearchTableParam>` is published as a nested schema and deserialized from
    a client's JSON, so it can accept an undeclared key exactly like the outer struct. Two
    passes cover the nesting this tree has; a fixed point is not worth the machinery.
    """
    names = params_referents(files)
    for _ in range(2):
        for raw in files.values():
            text = blank_inline_tests(raw)
            for name, _decl, body, _start in struct_items(text):
                if name not in names:
                    continue
                for f in FIELD_DECL.finditer(body):
                    for token in re.findall(r"[A-Za-z0-9_]+", f.group(2)):
                        if token.endswith(("Params", "Param")):
                            names.add(token)
    return names


def is_params_struct(name: str, attrs: str, referents: set[str]) -> bool:
    """A struct that a tool deserializes a client's arguments into.

    Three conditions, because each one alone is wrong: the name (`…Params`, or `…Param` for the
    element type of a list parameter), the two derives that make it one (`Deserialize` reads the
    arguments, `JsonSchema` publishes them — an output struct derives `Serialize` instead), and
    an actual `Parameters<…>` use.
    """
    if not (name.endswith("Params") or name.endswith("Param")):
        return False
    if not ("Deserialize" in attrs and "JsonSchema" in attrs):
        return False
    return name in referents


def undeclared_params_findings(files: dict[str, str]) -> list[Finding]:
    """The three shapes, over a `{path: text}` mapping so a canary can pass a sample."""
    found: list[Finding] = []
    referents = schema_bearing_params(files)
    for path, raw in sorted(files.items()):
        # Comments quote `Parameters<AnyParams>` while explaining why it is gone — mask them
        # or the explanation is reported as the defect.
        code = mask_comments(blank_inline_tests(raw))

        def line_of(offset: int) -> int:
            return code.count("\n", 0, offset) + 1

        for m in OPEN_REFERENT.finditer(code):
            found.append(
                Finding(
                    "undeclared-params",
                    f"{path}:{line_of(m.start())}",
                    f"takes `Parameters<{m.group(1)}>`, which reflects to a client as "
                    '`{"type": "object"}` — no properties, no types, no enums, and every '
                    "misspelled key silently dropped. Declare a params struct with "
                    "`#[derive(Deserialize, JsonSchema)]` and `#[serde(deny_unknown_fields)]`.",
                )
            )

        for name, decl_start, body, body_start in struct_items(code):
            attrs = attr_run(code, decl_start)
            if not is_params_struct(name, attrs, referents):
                continue
            if "deny_unknown_fields" not in attrs:
                found.append(
                    Finding(
                        "undeclared-params",
                        f"{path}:{line_of(decl_start)}",
                        f"`{name}` declares its parameters without "
                        "`#[serde(deny_unknown_fields)]`, so the schema says "
                        "`additionalProperties` is allowed and a misspelled key is accepted "
                        "and ignored. That is the state the phantom `max_chars` parameter "
                        "shipped in.",
                    )
                )
            for f in FIELD_DECL.finditer(body):
                fattrs = attr_run(body, f.start())
                if "flatten" not in fattrs:
                    continue
                if not FLATTEN_INTO_ANYTHING.search(f.group(2)):
                    continue
                found.append(
                    Finding(
                        "undeclared-params",
                        f"{path}:{line_of(body_start + f.start())}",
                        f"`{name}.{f.group(1)}` flattens `{f.group(2).strip()}` into the "
                        "parameter set, which accepts every key again while the struct still "
                        "looks closed. Name the parameters instead.",
                    )
                )
    return found


# ---------------------------------------------------------------------------
# prose-only-enum
#
# Shipped instance: `iris_system_performance` documented `mode: start | status | last_runid`
# and `iris_admin` branched on fourteen `action` values, and in both cases the fixed value set
# existed only in the English description. A client could not validate a value, could not offer
# completion, and a wrong value cost a round-trip to find out. Worse, the GEPA optimizer
# rewrites exactly that prose, so the only copy of the value set was the copy being edited by a
# tool with no idea it was load-bearing.
#
# This is the measurable half of SC-004: a description that spells out a value set for a
# parameter must be backed by that parameter advertising the set. Only two shapes are read as a
# value set — `name: a | b | c`, and a comma list introduced by "one of" — because English prose
# is full of commas and a looser pattern would flag sentences instead of contracts.
#
# `profile` on `iris_system_performance` is the case that proves the exemption is needed: IRIS
# ships six profiles and an instance may define more, so the set is not fixed. Saying "not an
# enum" in the field's doc comment is how that gets recorded — a sentence in the source, in
# review, rather than a silent omission.
# ---------------------------------------------------------------------------

TOOL_ATTR = re.compile(r"#\[tool\(")
TOOL_DESC = re.compile(r'description\s*=\s*"((?:[^"\\]|\\.)*)"')
TOOL_FN = re.compile(r"async fn ([a-z0-9_]+)\s*\(")
PARAMS_REFERENT = re.compile(r"Parameters<([A-Za-z0-9_:]+)>")
ENUM_DECL = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?enum[ \t]+([A-Za-z0-9_]+)", re.M
)

# `mode: start | status | last_runid` — a parameter name, then a pipe-separated token list.
PIPE_VALUES = re.compile(
    r"`?([a-z_][a-z0-9_]*)`?[ \t]*(?:\(optional\)[ \t]*)?[:=][ \t]*"
    r"(`?\"?[A-Za-z0-9_.\-]+\"?`?(?:[ \t]*\|[ \t]*`?\"?[A-Za-z0-9_.\-]+\"?`?)+)"
)
# `profile: for mode=start, one of test, 30mins, 4hours` — a comma list with an explicit
# lead-in. Without the lead-in this would match any sentence containing a comma.
ONE_OF_VALUES = re.compile(
    r"`?([a-z_][a-z0-9_]*)`?[^.]{0,60}?\b(?:one of|valid values(?: are)?)\b[ \t]*:?[ \t]*"
    r"((?:`?[A-Za-z0-9_.\-]+`?[^,.]{0,40},[ \t]*)+(?:or[ \t]+)?`?[A-Za-z0-9_.\-]+`?)"
)


def tool_blocks(text: str):
    """Yield (tool_name, description, params_struct, line) for every `#[tool(...)]` handler.

    The tool name is the handler's function name, which is what rmcp advertises. The params
    struct is the referent in the signature, reduced to its last path segment — the batch
    modules are addressed as `params::batch4::X` at the call site and declared as `X`.
    """
    starts = [m.start() for m in TOOL_ATTR.finditer(text)] + [len(text)]
    for i in range(len(starts) - 1):
        seg = text[starts[i] : starts[i + 1]]
        desc = TOOL_DESC.search(seg)
        fn = TOOL_FN.search(seg)
        if not desc or not fn:
            continue
        open_brace = seg.find("{", fn.end())
        sig = seg[fn.end() : open_brace if open_brace > 0 else len(seg)]
        ref = PARAMS_REFERENT.search(sig)
        yield (
            fn.group(1),
            desc.group(1).replace('\\"', '"'),
            ref.group(1).split("::")[-1] if ref else None,
            text.count("\n", 0, starts[i] + desc.start()) + 1,
        )


def advertised_value_sets(description: str) -> dict[str, str]:
    """Parameter name -> the value list the description spells out for it."""
    sets: dict[str, str] = {}
    for pattern in (PIPE_VALUES, ONE_OF_VALUES):
        for m in pattern.finditer(description):
            sets.setdefault(m.group(1), m.group(2).strip().rstrip("."))
    return sets


def params_field_index(
    files: dict[str, str],
) -> tuple[dict[str, dict[str, str]], set[str]]:
    """`{struct: {field: attrs+type}}` and the set of enum type names declared in the tree.

    Doc comments are kept here: the "not an enum" exemption is written in one, so this is the
    one index that must read them.
    """
    index: dict[str, dict[str, str]] = {}
    enums: set[str] = set()
    referents = schema_bearing_params(files)
    for raw in files.values():
        text = blank_inline_tests(raw)
        enums.update(m.group(1) for m in ENUM_DECL.finditer(text))
        for name, decl_start, body, _ in struct_items(text):
            if not is_params_struct(name, attr_run(text, decl_start), referents):
                continue
            fields = index.setdefault(name, {})
            for f in FIELD_DECL.finditer(body):
                fields[f.group(1)] = attr_run(body, f.start()) + "\n" + f.group(2)
    return index, enums


def prose_only_enum_findings(files: dict[str, str]) -> list[Finding]:
    index, enums = params_field_index(files)
    found: list[Finding] = []
    for path, raw in sorted(files.items()):
        code = mask_comments(blank_inline_tests(raw))
        for tool, description, struct, line in tool_blocks(code):
            fields = index.get(struct or "")
            if fields is None:
                continue
            for param, values in sorted(advertised_value_sets(description).items()):
                declared = fields.get(param)
                # A parameter the description names and the struct does not is a different
                # bug, and `test_docs_contract.rs` already fails on it.
                if declared is None:
                    continue
                if 'extend("enum"' in declared:
                    continue
                if any(re.search(rf"\b{e}\b", declared) for e in enums):
                    continue
                if "not an enum" in declared:
                    continue
                found.append(
                    Finding(
                        "prose-only-enum",
                        f"{path}:{line}",
                        f"`{tool}` documents a fixed value set for `{param}` "
                        f"({values}) that the parameter does not advertise. The prose is the "
                        "only copy of the contract, so a client cannot validate a value and "
                        "the description optimizer can delete the set without noticing. Add "
                        '`#[schemars(extend("enum" = [...]))]` to '
                        f"`{struct}.{param}`, or say in its doc comment why this is `not an "
                        'enum" (the way `profile` does).',
                    )
                )
    return found


def _src_texts() -> dict[str, str]:
    return {rel(p): p.read_text(errors="replace") for p in src_files()}


def check_undeclared_params() -> list[Finding]:
    return undeclared_params_findings(_src_texts())


def check_prose_only_enum() -> list[Finding]:
    return prose_only_enum_findings(_src_texts())


CHECKS = {
    "vacuous-tests": check_vacuous_tests,
    "empty-tests": check_empty_tests,
    "mcp-subcommand": check_mcp_subcommand,
    "env-pinning": check_env_pinning,
    "error-sentinels": check_error_sentinels,
    "device-capture": check_device_capture,
    "self-referential-gates": check_self_referential_gates,
    "version-consistency": check_version_consistency,
    "tool-name-refs": check_tool_name_refs,
    "binary-path": check_binary_path,
    "empty-config-value": check_empty_config_value,
    "stale-coverage-objects": check_stale_coverage_objects,
    "undeclared-params": check_undeclared_params,
    "prose-only-enum": check_prose_only_enum,
}

# Findings in these classes always fail the gate, baseline or not: the class is fully
# cleaned up in the tree, so any instance is new by definition.
NO_BASELINE = {
    "error-sentinels",
    "self-referential-gates",
    "version-consistency",
    "binary-path",
    "empty-config-value",
    "stale-coverage-objects",
    "undeclared-params",
    "prose-only-enum",
}


def load_baseline() -> set[str]:
    if not BASELINE.exists():
        return set()
    return {
        ln.strip()
        for ln in BASELINE.read_text().splitlines()
        if ln.strip() and not ln.startswith("#")
    }


def main(argv: list[str]) -> int:
    args = [a for a in argv if not a.startswith("--")]
    flags = {a for a in argv if a.startswith("--")}
    names = args or list(CHECKS)
    for n in names:
        if n not in CHECKS:
            print(
                f"antipatterns: unknown check: {n} (have: {' '.join(CHECKS)})",
                file=sys.stderr,
            )
            return 1

    findings: list[Finding] = []
    for n in names:
        findings.extend(CHECKS[n]())
    findings.sort(key=lambda f: (f.check, f.location))

    if "--write-baseline" in flags:
        header = [
            "# Known instances of each antipattern, recorded when its detector was written.",
            "#",
            "# The gate fails on a finding that is NOT in this file, and on a line in this",
            "# file that no longer fires. That is what makes the list shrink instead of rot.",
            "# Regenerate with: scripts/gates/antipatterns.py --write-baseline",
            "#",
            "# Adding a line here silences a real finding. It is a tracked edit and it will",
            "# show up in review — say why in the commit message.",
            "",
        ]
        BASELINE.write_text(
            "\n".join(
                header + [f.key() for f in findings if f.check not in NO_BASELINE]
            )
            + "\n"
        )
        print(
            f"antipatterns: wrote {BASELINE.relative_to(ROOT)} ({len(findings)} lines)"
        )
        return 0

    if "--all-findings" in flags:
        for f in findings:
            print(f"FINDING [{f.check}] {f.location}\n    {f.message}")
        print(f"\n{len(findings)} finding(s) total (baseline ignored).")
        return 0

    baseline = load_baseline()
    seen = {f.key() for f in findings}
    new = [f for f in findings if f.key() not in baseline or f.check in NO_BASELINE]
    # A baseline entry that no longer fires is a fixed bug whose line was never removed.
    # Only reconcile entries for the checks that ran, or a single-check run looks stale.
    ran = set(names)
    stale = sorted(k for k in baseline - seen if k.split("\t", 1)[0] in ran)

    for f in new:
        print(f"FINDING [{f.check}] {f.location}\n    {f.message}")
    for k in stale:
        check, loc = k.split("\t", 1)
        print(f"STALE BASELINE [{check}] {loc}")
        print(
            "    no longer fires. Delete this line from "
            f"{BASELINE.relative_to(ROOT)} — the baseline only shrinks."
        )

    if new or stale:
        print(
            f"\n{len(new)} new finding(s), {len(stale)} stale baseline line(s). "
            "Each check maps to a row in the Bug Class Registry in "
            ".specify/memory/constitution.md."
        )
        return 2

    print(
        f"antipatterns: clean ({' '.join(names)}) — "
        f"{len(baseline)} known instance(s) still in the baseline"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
