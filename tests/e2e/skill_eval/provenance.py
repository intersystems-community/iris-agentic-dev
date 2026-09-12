"""What a measurement was taken under — 118 T005.

Two jobs, and they are here together because the second needs the first:

1. Find the `iris-agentic-dev` binary the agent sessions should run. This used to be a
   string literal in `isolated_env.py` pointing at a Homebrew path, so on a GitHub runner
   every session started with no iad tools at all and the harness recorded the resulting
   zeros as skill measurements for a month.
2. Record the tool surface, scorer model, and task set a number was measured under, so a
   later run can say whether its number is comparable rather than assuming it is.

`tool --list --json` reads the tool router and makes no IRIS connection, so the surface
resolves on a machine with no container.
"""

from __future__ import annotations

import dataclasses
import datetime
import hashlib
import json
import os
import shutil
import subprocess
from typing import Optional

HOMEBREW_FALLBACK = "/opt/homebrew/bin/iris-agentic-dev"

# The binary answers both of these without touching IRIS.
_LIST_ARGS = ("tool", "--list", "--json")
_VERSION_ARGS = ("--version",)

_TIMEOUT_S = 20


@dataclasses.dataclass(frozen=True)
class BinaryCandidate:
    """One place the binary was looked for, and whether it was there."""

    source: str  # "IAD_BINARY" | "PATH" | "fallback"
    path: Optional[str]
    usable: bool

    def describe(self) -> str:
        where = self.path or "(unset)"
        return f"{self.source}: {where} {'ok' if self.usable else '(not found)'}"


def _usable(path: Optional[str]) -> bool:
    return bool(path) and os.path.isfile(path) and os.access(path, os.X_OK)


def binary_candidates() -> list[BinaryCandidate]:
    """The three places, in order, each with its verdict.

    Reported in full rather than short-circuited: the preflight's job is to tell someone
    which of the three to fix, and it cannot do that from a bare `None`.
    """
    explicit = os.environ.get("IAD_BINARY") or None
    on_path = shutil.which("iris-agentic-dev")
    return [
        BinaryCandidate("IAD_BINARY", explicit, _usable(explicit)),
        BinaryCandidate("PATH", on_path, _usable(on_path)),
        BinaryCandidate("fallback", HOMEBREW_FALLBACK, _usable(HOMEBREW_FALLBACK)),
    ]


def resolve_binary() -> Optional[str]:
    """The first usable binary, or None.

    An `IAD_BINARY` that does not resolve stops resolution instead of falling through. CI
    sets it to the release artifact it means to measure; quietly measuring a different
    build would produce a number that answers a question nobody asked.
    """
    candidates = binary_candidates()
    if candidates[0].path is not None:
        return candidates[0].path if candidates[0].usable else None
    for candidate in candidates[1:]:
        if candidate.usable:
            return candidate.path
    return None


def _run(binary: str, args: tuple[str, ...]) -> Optional[str]:
    try:
        proc = subprocess.run(
            [binary, *args],
            capture_output=True,
            text=True,
            timeout=_TIMEOUT_S,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if proc.returncode != 0:
        return None
    return proc.stdout


def binary_version(binary: Optional[str]) -> Optional[str]:
    """The version string alone, e.g. `1.4.1` from `iris-agentic-dev 1.4.1`."""
    if not binary:
        return None
    out = _run(binary, _VERSION_ARGS)
    if not out:
        return None
    parts = out.strip().split()
    return parts[-1] if parts else None


def tool_list(binary: Optional[str]) -> Optional[list[dict]]:
    """The `{name, summary}` records the binary advertises, or None if it would not say."""
    if not binary:
        return None
    out = _run(binary, _LIST_ARGS)
    if not out:
        return None
    try:
        payload = json.loads(out)
    except json.JSONDecodeError:
        return None
    tools = payload.get("tools")
    return tools if isinstance(tools, list) else None


def surface_hash(tools: list[dict]) -> Optional[str]:
    """12 hex over the sorted `name\tsummary` pairs, or None for an empty surface.

    Sorted, because the router's ordering is not part of the contract and two runs must not
    look like two surfaces over a shuffled list. Summaries included, because descriptions
    are what the GEPA optimizer edits — a lift that moved because a description was
    rewritten and one that moved because a skill changed should be distinguishable in the
    record. Nothing gates on the value; a differing surface annotates a comparison.
    """
    if not tools:
        return None
    lines = sorted(f"{t.get('name', '')}\t{t.get('summary', '')}" for t in tools)
    return hashlib.sha256("\n".join(lines).encode()).hexdigest()[:12]


def tool_surface(binary: Optional[str]) -> str:
    """`<version>+<12 hex>`, or the literal `"none"`.

    `"none"` is a measurement fact, not a missing value: it says the session ran without
    iad tools. That is exactly the state the 2026-08 nightlies were in, and it should be
    legible in the baseline file rather than absent from it.
    """
    tools = tool_list(binary)
    digest = surface_hash(tools or [])
    version = binary_version(binary)
    if digest is None or version is None:
        return "none"
    return f"{version}+{digest}"


def harness_commit() -> Optional[str]:
    """Short SHA of the harness tree, when git can say. A tarball checkout cannot."""
    try:
        proc = subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"],
            capture_output=True,
            text=True,
            timeout=_TIMEOUT_S,
            cwd=os.path.dirname(os.path.abspath(__file__)),
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if proc.returncode != 0:
        return None
    return proc.stdout.strip() or None


def _utc_now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


@dataclasses.dataclass
class Provenance:
    """data-model.md § 3. What makes an old number comparable, or explains why it is not."""

    run_id: str
    task_ids: list[str]
    scoring_mode: str
    scorer_model: Optional[str]
    scorer_model_requested: Optional[str]
    tool_surface: str
    runs: int
    measured_at: str = dataclasses.field(default_factory=_utc_now)
    harness_commit: Optional[str] = dataclasses.field(default_factory=harness_commit)

    def __post_init__(self) -> None:
        # Sorted here rather than at every call site: the task set is compared for equality,
        # and two orderings of one corpus are one corpus.
        self.task_ids = sorted(self.task_ids)

    def to_dict(self) -> dict:
        return dataclasses.asdict(self)

    @classmethod
    def from_dict(cls, data: dict) -> "Provenance":
        fields = {f.name for f in dataclasses.fields(cls)}
        return cls(**{k: v for k, v in data.items() if k in fields})
