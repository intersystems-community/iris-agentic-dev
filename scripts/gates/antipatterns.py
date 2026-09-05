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


def check_empty_tests() -> list[Finding]:
    found = []
    for path in src_files() + test_files():
        text = path.read_text(errors="replace")
        for name, line, body, attrs in test_fns(text):
            code = strip_comments(body)
            if ASSERTS.search(code):
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
                    f"{rel(path)}:{line}",
                    f"`{name}` asserts nothing — no assert!, unwrap, expect, or `?`. It "
                    "reports ok without checking anything. Assert something, or delete it "
                    "and put the reason in a comment.",
                )
            )
    return found


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


def check_device_capture() -> list[Finding]:
    found = []
    for path in src_files():
        # A gate's whole job is to name the dangerous APIs. `src/policy/` holds blocklist
        # token constants and the doc comments explaining them — nothing there is ever handed
        # to IRIS, so a hit is guaranteed noise. Noise is what teaches people to bypass a
        # gate, which is the failure this suite exists to prevent.
        if "/src/policy/" in rel(path):
            continue
        text = blank_inline_tests(path.read_text(errors="replace"))
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
                    f"{rel(path)}:{i}",
                    f"calls `{hit.group(0)}`, which can switch the current device, with no "
                    "$IO snapshot within 20 lines. Wrap it: `Set tIO=$IO` / call / "
                    "`Use tIO`, then Write. Otherwise the output lands on the callee's "
                    "device and the tool sees an empty success.",
                )
            )
    return found


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
