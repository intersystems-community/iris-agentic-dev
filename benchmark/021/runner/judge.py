"""LLM-as-judge scoring using Claude Haiku as arbiter (Bedrock or direct API)."""

import json

try:
    from ._client import CREDENTIAL_VARS, haiku_model, make_client, resolved_model
except ImportError:
    from _client import CREDENTIAL_VARS, haiku_model, make_client, resolved_model

RUBRIC = """You are evaluating an AI coding agent's performance on an IRIS development task.

TASK: {description}
EXPECTED: {expected_behavior}
PATH: Path {path} ({path_label})

AGENT TRANSCRIPT (tool calls and final response):
{transcript}

Score the agent 0-3:
0 = Failed or wrong output (did not compile/load, wrong behavior, gave up)
1 = Partial — file exists but incorrect behavior or missing required elements
2 = Correct but required more than 2 unnecessary tool calls (agent confusion)
3 = Correct and efficient (right output, minimal tool calls)

{category_note}

Return ONLY valid JSON with no other text: {{"score": <0-3>, "reasoning": "<one sentence>"}}"""

PYPR_CATEGORY_NOTE = """This is a PYTHON/pyprod task. Important facts for scoring:
- intersystems_pyprod exports: IRISProperty, IRISParameter, BusinessService, BusinessProcess,
  BusinessOperation, InboundAdapter, OutboundAdapter, Column, JsonSerialize, PickleSerialize,
  IRISLog, Status, Production, ServiceItem, ProcessItem, OperationItem — all are valid
- `return Status.OK(), None` is the CORRECT return form for message handlers (tuple required)
- `return Status.OK()` (bare, no tuple) is only correct for on_message fallback
- Path A/B distinctions (iris_doc, iris_compile, local files) do NOT apply to Python tasks —
  Python files are not compiled into IRIS; path is irrelevant for scoring pyprod code generation
- Do NOT penalize for not calling iris_doc or iris_compile on a Python code generation task"""

DOC_CATEGORY_NOTE = """This is a DOCUMENTATION RETRIEVAL task. Scoring rules:
- Score 3: all key facts in expected_behavior present and correct; method/class names exact
- Score 2: mostly correct but agent used more than 2 unnecessary tool calls, OR missed one fact
- Score 1: partially correct — right concept but wrong detail (e.g. wrong method name, hallucinated class)
- Score 0: hallucinated API that doesn't exist, or missing the core answer entirely
CRITICAL: Penalize hallucinated method names (e.g. CheckPermission, HasPermission, $VERSION) even if
the surrounding answer sounds plausible. The exact name matters — wrong name = score 1 at most.
Path A/B distinction does NOT apply to documentation tasks — ignore path labels for scoring."""

PATH_LABELS = {
    "A": "Local Files + Atelier — agent edits local .cls files, uses iris_compile",
    "B": "ISFS Only — agent uses iris_doc to read/write, no local files",
}


def unscored(reason: str) -> dict:
    """The verdict for an item the scorer did not score.

    `score: None`, never `0`. A zero is a measurement — the model read the transcript and
    rejected it — and this function exists for the case where no model read anything. The
    two used to be the same value, so the 2026-08 nightlies published nine skills at a 0%
    pass rate that nothing had scored.
    """
    return {
        "scored": False,
        "score": None,
        "reasoning": reason,
        "scoring_mode": "judge",
        "scorer_model": None,
        "input_tokens": None,
        "output_tokens": None,
    }


def _usage(msg) -> tuple:
    """Token counts from the response, or `(None, None)` if it carries none.

    Missing usage costs the cost estimate, not the measurement — the score is still real.
    """
    usage = getattr(msg, "usage", None)
    if usage is None:
        return None, None
    return getattr(usage, "input_tokens", None), getattr(usage, "output_tokens", None)


def _validated_score(raw):
    """The score if it is on the 0–3 scale, else a message saying why it is not.

    Returns `(score, None)` or `(None, reason)`. `7` is not clamped to `3`: an unmeasured
    number in the middle of the scale is the same lie as a fabricated zero and harder to
    spot. `bool` is excluded because `True` is an `int` and is not a verdict.
    """
    if isinstance(raw, bool) or not isinstance(raw, int):
        return None, f"scorer returned {raw!r}, which is not a 0-3 score"
    if raw not in (0, 1, 2, 3):
        return None, f"scorer returned {raw!r}, out of the 0-3 range"
    return raw, None


def score_result(task: dict, result: dict) -> dict:
    """Score a task result with the judge model. Returns the verdict in contracts/scoring.md.

    Never raises for a scorer failure and never returns a score it did not get from the
    model: an unreachable scorer, an unparseable answer, and an off-scale number all come
    back `scored: False`.
    """
    transcript = _format_transcript(result.get("transcript", []))
    category = task.get("category", "")
    if category == "PYPR":
        category_note = PYPR_CATEGORY_NOTE
    elif category == "DOC":
        category_note = DOC_CATEGORY_NOTE
    else:
        category_note = ""
    prompt = RUBRIC.format(
        description=task["description"],
        expected_behavior=task.get("expected_behavior", "(see description)"),
        path=result.get("path", "A"),
        path_label=PATH_LABELS.get(result.get("path", "A"), ""),
        transcript=transcript,
        category_note=category_note,
    )

    try:
        client = make_client()
    except Exception as e:
        return unscored(
            f"scorer unreachable: client could not be built ({e}). Looked for "
            f"{', '.join(CREDENTIAL_VARS)}"
        )

    reason = "scorer made no attempt"
    for attempt in range(2):
        try:
            msg = client.messages.create(
                model=haiku_model(),
                max_tokens=256,
                messages=[{"role": "user", "content": prompt}],
            )
        except Exception as e:
            # Retried once: Bedrock throttles. Both attempts failing is a real failure,
            # and a failure has no score.
            reason = (
                f"scorer unreachable: {haiku_model()} call failed ({e}). Looked for "
                f"{', '.join(CREDENTIAL_VARS)}"
            )
            continue

        text = str(msg.content[0].text).strip()
        try:
            parsed = json.loads(text)
        except Exception:
            reason = f"scorer answer did not parse as JSON: {text[:200]!r}"
            continue
        if not isinstance(parsed, dict) or "score" not in parsed:
            reason = f"scorer answer carried no score: {text[:200]!r}"
            continue
        score, problem = _validated_score(parsed["score"])
        if problem:
            reason = problem
            continue

        input_tokens, output_tokens = _usage(msg)
        return {
            "scored": True,
            "score": score,
            "reasoning": parsed.get("reasoning", ""),
            "scoring_mode": "judge",
            # Never the requested constant on its own: that is how every stored number came
            # to be attributed to a Haiku id while Bedrock served Sonnet.
            "scorer_model": resolved_model(msg)
            or f"unreported (requested {haiku_model()})",
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        }

    return unscored(reason)


def _format_transcript(turns: list) -> str:
    lines = []
    for turn in turns:
        role = turn.get("role", "?")
        if turn.get("tool_name"):
            lines.append(
                f"[{role}] tool_call: {turn['tool_name']}({json.dumps(turn.get('args', {}))[:120]})"
            )
        if turn.get("tool_result"):
            lines.append(f"[tool_result] {str(turn['tool_result'])[:200]}")
        if turn.get("text"):
            lines.append(f"[{role}] {turn['text'][:8000]}")
    return "\n".join(lines) if lines else "(empty transcript)"
