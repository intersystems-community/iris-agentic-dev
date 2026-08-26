"""Write benchmark results incrementally to JSON and generate HTML report."""

import json
import os
import datetime
from pathlib import Path


class ResultWriter:
    def __init__(self):
        ts = datetime.datetime.utcnow().strftime("%Y-%m-%dT%H-%M-%SZ")
        base = Path(__file__).parent.parent / "results" / ts
        base.mkdir(parents=True, exist_ok=True)
        self.run_dir = str(base)
        self.scores_path = str(base / "scores.json")
        self.report_path = str(base / "report.html")
        self._run = {
            "run_id": ts,
            "iris_dev_version": _get_version(),
            # Both arms of the 085 A/B report version 1.2.6 — the branch is unreleased — so the
            # resolved path is the only thing that tells the two runs apart in the artifact.
            "iris_dev_path": _get_binary_path(),
            "tasks": [],
            "summary": {},
        }
        self._flush()

    def record(
        self,
        task_id: str,
        category: str,
        path: str,
        harness: str,
        scored: dict,
        result: dict,
        condition: str = "baseline",
    ):
        entry = {
            "task_id": task_id,
            "category": category,
            "path": path,
            "harness": harness,
            "condition": condition,
            "score": scored["score"],
            "reasoning": scored.get("reasoning", ""),
            "tool_call_count": result.get("tool_call_count", 0),
            "stub_error_count": result.get("stub_error_count", 0),
            "wrong_tool_count": result.get("wrong_tool_count", 0),
            "scm_elicitation_triggered": _scm_triggered(result.get("transcript", [])),
            "gate_refusal_count": _gate_refusals(result.get("transcript", [])),
        }
        self._run["tasks"].append(entry)
        self._flush()
        self._write_transcript(task_id, path, harness, result)

    def _write_transcript(self, task_id: str, path: str, harness: str, result: dict):
        """Persist the raw transcript next to scores.json.

        A score on its own cannot answer whether a write/destructive gate refused a call
        mid-task, which is the question spec 085's release gate (T068) exists to answer.
        """
        tdir = Path(self.run_dir) / "transcripts"
        tdir.mkdir(parents=True, exist_ok=True)
        with open(tdir / f"{task_id}_{path}_{harness}.json", "w") as f:
            json.dump(result.get("transcript", []), f, indent=2)

    def set_condition_metadata(self, condition: str, wall_clock_seconds: float):
        self._run["condition"] = condition
        self._run["wall_clock_seconds"] = wall_clock_seconds

    def finalize(self):
        self._run["summary"] = _compute_summary(self._run["tasks"])
        self._flush()
        self._write_html()
        print(f"scores.json  → {self.scores_path}")
        print(f"report.html  → {self.report_path}")

    def _flush(self):
        with open(self.scores_path, "w") as f:
            json.dump(self._run, f, indent=2)

    def _write_html(self):
        from .report import generate_report

        generate_report(self.scores_path, self.report_path)


def _get_binary_path() -> str:
    import shutil

    resolved = shutil.which("iris-dev") or "iris-dev"
    try:
        # A PATH shim is how a specific build gets measured; report what it execs, not the shim.
        with open(resolved) as f:
            head = f.read(4096)
        for line in head.splitlines():
            if line.startswith("exec ") and os.path.exists(line.split()[1]):
                return f"{resolved} -> {line.split()[1]}"
    except Exception:
        pass
    return resolved


def _get_version() -> str:
    import subprocess

    try:
        r = subprocess.run(["iris-dev", "--version"], capture_output=True, text=True)
        return r.stdout.strip().split()[-1]
    except Exception:
        return "unknown"


# The two error codes the 085 gate returns. Counted per task so a run can prove no call was
# refused, rather than inferring it from the score.
_GATE_ERROR_CODES = ("WRITE_TOOLS_DISABLED", "DESTRUCTIVE_TOOLS_DISABLED")


def _gate_refusals(transcript: list) -> int:
    return sum(
        1
        for t in transcript
        if any(code in str(t.get("tool_result", "")) for code in _GATE_ERROR_CODES)
    )


def _scm_triggered(transcript: list) -> bool:
    return any(
        t.get("tool_name") == "iris_source_control"
        or "elicitation" in str(t.get("tool_result", "")).lower()
        for t in transcript
    )


def _compute_summary(tasks: list) -> dict:
    if not tasks:
        return {}

    scores_a = [t["score"] for t in tasks if t["path"] == "A"]
    scores_b = [t["score"] for t in tasks if t["path"] == "B"]

    by_category = {}
    for t in tasks:
        cat = t["category"]
        if cat not in by_category:
            by_category[cat] = {"A": [], "B": []}
        by_category[cat][t["path"]].append(t["score"])

    return {
        "mean_score_path_a": _mean(scores_a),
        "mean_score_path_b": _mean(scores_b),
        "task_count": len(tasks),
        "by_category": {
            cat: {
                "path_a": _mean(v["A"]),
                "path_b": _mean(v["B"]),
            }
            for cat, v in by_category.items()
        },
    }


def _mean(vals: list) -> float:
    return round(sum(vals) / len(vals), 2) if vals else 0.0
