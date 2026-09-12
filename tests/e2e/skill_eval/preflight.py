"""Preflight: find out before spending anything — 118 T017.

The 2026-08 nightly spent roughly $3.70 and two hours a night running nine skills whose scorer
it could not reach, and published the result as a 0% pass rate. Four checks, cheapest first, all
of them before the first agent session:

1. the task corpus parses and every task a skill names exists
2. the scorer answers — exactly one real call, so a broken credential costs one call and not nine
3. a binary resolves
4. that binary serves a tool surface

Contract: `specs/118-skill-eval-harness-repair/contracts/scoring.md` § Preflight. The order is
part of it: a run with a broken corpus *and* a broken credential reports the corpus, because
fixing the credential would not have helped.
"""

import glob
import json
import os
from dataclasses import dataclass, field
from typing import Optional

import yaml

# Triggers the sys.path shim that makes `runner` importable.
import tests.e2e.skill_eval  # noqa: F401
from tests.e2e.skill_eval import provenance
from runner._client import CREDENTIAL_VARS, auth_source, haiku_model, make_client  # noqa: E402

_TARGETED_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "tasks", "skills", "targeted")
)
_BENCHMARK_TASKS_DIR = os.path.abspath(
    os.path.join(
        os.path.dirname(__file__), "..", "..", "..", "benchmark", "021", "tasks"
    )
)
_TASKS_SKILLS_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "tasks", "skills")
)

# Keys that are commonly present and are not the scorer's. Naming them is the difference
# between "no credential" and an afternoon spent rotating the wrong secret.
UNUSED_KEYS = ("OPENAI_API_KEY",)

# A two-line transcript with an obvious verdict. Short on purpose: the answer is not used for
# anything except proving the scorer can produce one.
PROBE_PROMPT = (
    "Score this trivial transcript 0-3 for correctness. "
    "The agent was asked for 2+2 and answered 4.\n"
    'Return ONLY valid JSON: {"score": <0-3>, "reasoning": "<one sentence>"}'
)
PROBE_MAX_TOKENS = 64


@dataclass
class ScorerProbe:
    """The result of the one scoring call the preflight makes."""

    ok: bool
    detail: str = ""
    model: Optional[str] = None
    input_tokens: Optional[int] = None
    output_tokens: Optional[int] = None
    raw_text: str = ""


@dataclass
class PreflightResult:
    ok: bool
    skipped: bool = False
    stage: Optional[str] = None
    failure: Optional[str] = None
    scorer_model: Optional[str] = None
    binary: Optional[str] = None
    tool_surface: Optional[str] = None
    scorer_model_requested: Optional[str] = None
    auth_source: Optional[str] = None
    stages_run: list = field(default_factory=list)


# ---------------------------------------------------------------------------
# 1. Corpus
# ---------------------------------------------------------------------------


def _eval_configs(skills=None):
    """Every `(skill, eval.yaml as a dict)` under `tests/e2e/tasks/skills/`.

    Deliberately not `load_eval_config`: the preflight validates files, so it reads them raw
    and reports a parse error as a parse error rather than as a missing config.
    """
    configs = []
    for path in sorted(glob.glob(os.path.join(_TASKS_SKILLS_DIR, "*", "eval.yaml"))):
        skill = os.path.basename(os.path.dirname(path))
        if skills and skill not in skills:
            continue
        with open(path) as f:
            try:
                configs.append((skill, yaml.safe_load(f) or {}))
            except yaml.YAMLError as e:
                configs.append((skill, {"_parse_error": f"{path}: {e}"}))
    return configs


def _task_path(task_id: str) -> Optional[str]:
    for directory in (_TARGETED_DIR, _BENCHMARK_TASKS_DIR):
        candidate = os.path.join(directory, f"{task_id}.yaml")
        if os.path.exists(candidate):
            return candidate
    return None


def validate_corpus(skills=None) -> Optional[str]:
    """`None` when the corpus is usable, else one line naming the task and the file.

    Costs nothing, so it runs first. A task file that does not parse fails the run 90 minutes
    earlier here than it does inside `run_task_and_score`.
    """
    for skill, cfg in _eval_configs(skills):
        if cfg.get("_parse_error"):
            return f"{skill}: eval.yaml did not parse — {cfg['_parse_error']}"
        for task_id in cfg.get("benchmark_tasks", []) or []:
            path = _task_path(task_id)
            if not path:
                return (
                    f"{skill}: task {task_id} is named in eval.yaml but no "
                    f"{task_id}.yaml exists under {_TARGETED_DIR} or "
                    f"{_BENCHMARK_TASKS_DIR}"
                )
            with open(path) as f:
                try:
                    task = yaml.safe_load(f)
                except yaml.YAMLError as e:
                    return f"{skill}: {task_id}: {path} is not valid YAML — {e}"
            if not isinstance(task, dict) or not task.get("description"):
                return (
                    f"{skill}: {task_id}: {path} has no description to send the agent"
                )
    return None


# ---------------------------------------------------------------------------
# 2. Scorer
# ---------------------------------------------------------------------------


def _credential_report() -> str:
    looked = ", ".join(
        f"{var} ({'set' if os.environ.get(var) else 'absent'})"
        for var in CREDENTIAL_VARS
    )
    present_unused = [k for k in UNUSED_KEYS if os.environ.get(k)]
    lines = [f"  looked for: {looked}"]
    if present_unused:
        lines.append(
            f"  present but unused: {', '.join(present_unused)} — that is the agent's key, "
            "not the scorer's"
        )
    lines.append(
        "  the scorer runs on Bedrock; set AWS_BEARER_TOKEN_BEDROCK and AWS_REGION"
    )
    lines.append("  nothing was spent")
    return "\n".join(lines)


def probe_scorer() -> ScorerProbe:
    """One real scoring call. One, not a retry loop.

    A retry here is the difference between one cent and nine on a credential that will never
    work, and the preflight's whole purpose is to be the cheap thing that runs first.
    """
    try:
        client = make_client()
    except Exception as e:
        return ScorerProbe(ok=False, detail=f"scorer client could not be built: {e}")

    try:
        msg = client.messages.create(
            model=haiku_model(),
            max_tokens=PROBE_MAX_TOKENS,
            messages=[{"role": "user", "content": PROBE_PROMPT}],
        )
    except Exception as e:
        return ScorerProbe(ok=False, detail=f"scoring call failed: {e}")

    text = str(msg.content[0].text).strip()
    model = getattr(msg, "model", None) or None
    usage = getattr(msg, "usage", None)
    input_tokens = getattr(usage, "input_tokens", None) if usage else None
    output_tokens = getattr(usage, "output_tokens", None) if usage else None

    try:
        parsed = json.loads(text)
        score = parsed["score"]
    except Exception:
        return ScorerProbe(
            ok=False,
            detail=(
                f"the scorer answered but not with a verdict: {text[:200]!r}. Reachable is "
                "not usable — a model that will not emit JSON scores nothing"
            ),
            model=model,
            raw_text=text,
        )
    if (
        isinstance(score, bool)
        or not isinstance(score, int)
        or score not in (0, 1, 2, 3)
    ):
        return ScorerProbe(
            ok=False,
            detail=f"the scorer answered with {score!r}, which is not a 0-3 score",
            model=model,
            raw_text=text,
        )

    return ScorerProbe(
        ok=True,
        model=model,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        raw_text=text,
    )


# ---------------------------------------------------------------------------
# The preflight itself
# ---------------------------------------------------------------------------


def _skips(args) -> bool:
    """The three modes that start no session and so need no credential.

    `--list-skills` builds the CI matrix. A matrix job that needed the scorer secret to
    enumerate directory names would fail the whole nightly on a secret rotation.
    """
    return bool(
        getattr(args, "list_skills", False)
        or getattr(args, "dry_run", False)
        or getattr(args, "merge_results", None)
    )


def preflight(args) -> PreflightResult:
    """Run the four checks in contract order. Never raises; the caller reads `.ok`."""
    if _skips(args):
        return PreflightResult(ok=True, skipped=True)

    stages: list = []
    skills = [args.skill] if getattr(args, "skill", None) else None

    stages.append("corpus")
    corpus_failure = validate_corpus(skills=skills)
    if corpus_failure:
        return PreflightResult(
            ok=False,
            stage="corpus",
            failure=f"task corpus is not usable\n  {corpus_failure}\n  nothing was spent",
            stages_run=stages,
        )

    stages.append("scorer")
    probe = probe_scorer()
    if not probe.ok:
        return PreflightResult(
            ok=False,
            stage="scorer",
            failure=f"scorer unreachable\n  {probe.detail}\n{_credential_report()}",
            scorer_model_requested=haiku_model(),
            auth_source=auth_source(),
            stages_run=stages,
        )

    stages.append("binary")
    binary = provenance.resolve_binary()
    if not binary:
        searched = "; ".join(c.describe() for c in provenance.binary_candidates())
        return PreflightResult(
            ok=False,
            stage="binary",
            failure=(
                "no iris-agentic-dev binary resolved, so every session would run with no "
                f"iad tools and score as though the skill failed. Searched — {searched}"
            ),
            scorer_model=probe.model,
            scorer_model_requested=haiku_model(),
            auth_source=auth_source(),
            stages_run=stages,
        )

    stages.append("tool_surface")
    surface = provenance.tool_surface(binary)
    if surface == "none":
        return PreflightResult(
            ok=False,
            stage="tool_surface",
            failure=(
                f"{binary} served no tool surface — `tool --list --json` returned nothing. "
                "That is the state the 2026-08 nightlies ran in: sessions with no iad tools, "
                "transcripts scored anyway, zeros published as skill measurements"
            ),
            scorer_model=probe.model,
            scorer_model_requested=haiku_model(),
            binary=binary,
            auth_source=auth_source(),
            stages_run=stages,
        )

    return PreflightResult(
        ok=True,
        scorer_model=probe.model,
        scorer_model_requested=haiku_model(),
        binary=binary,
        tool_surface=surface,
        auth_source=auth_source(),
        stages_run=stages,
    )
