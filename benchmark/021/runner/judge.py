"""LLM-as-judge scoring using Claude Haiku as arbiter (Bedrock or direct API)."""
import json
try:
    from ._client import make_client, haiku_model
except ImportError:
    from _client import make_client, haiku_model

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


def score_result(task: dict, result: dict) -> dict:
    """Score a task result using Claude Haiku as judge. Returns {score, reasoning}."""
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

    client = make_client()
    for attempt in range(2):
        try:
            msg = client.messages.create(
                model=haiku_model(),
                max_tokens=256,
                messages=[{"role": "user", "content": prompt}],
            )
            text = msg.content[0].text.strip()
            parsed = json.loads(text)
            score = int(parsed["score"])
            if score not in (0, 1, 2, 3):
                raise ValueError(f"score out of range: {score}")
            return {"score": score, "reasoning": parsed.get("reasoning", "")}
        except Exception as e:
            if attempt == 1:
                return {"score": 0, "reasoning": f"Judge error: {e}"}

    return {"score": 0, "reasoning": "Judge failed after retries"}


def _format_transcript(turns: list) -> str:
    lines = []
    for turn in turns:
        role = turn.get("role", "?")
        if turn.get("tool_name"):
            lines.append(f"[{role}] tool_call: {turn['tool_name']}({json.dumps(turn.get('args', {}))[:120]})")
        if turn.get("tool_result"):
            lines.append(f"[tool_result] {str(turn['tool_result'])[:200]}")
        if turn.get("text"):
            lines.append(f"[{role}] {turn['text'][:8000]}")
    return "\n".join(lines) if lines else "(empty transcript)"
