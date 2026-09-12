"""Scored items, pass rates, and what an invalid run is allowed to do — 118 T015.

One rule holds this module together: a rate computed over items nobody scored is not a rate.
The harness used to substitute `0` for an unscorable item, so `0.00` in a report meant either
"the skill failed every task" or "nothing was measured", and the nightly published the second
as the first for a month.

Contracts: `specs/118-skill-eval-harness-repair/contracts/scoring.md`, and the Scored-item and
Arm-result tables in that feature's `data-model.md`.
"""

from dataclasses import dataclass
from typing import Iterable, Optional, Union

VALID_SCORES = (0, 1, 2, 3)
PASS_THRESHOLD = 2

# The share of unscorable items above which the run does not describe the skill any more.
# "Exceeds" is the contract, so exactly this value is still a valid run.
UNSCORED_LIMIT = 0.10

# 0 measured something. 1 ran and cannot be trusted. 2 stopped before spending anything —
# the distinction matters when someone is reading a failed nightly and deciding whether the
# bill moved.
EXIT_MEASURED = 0
EXIT_INTEGRITY = 1
EXIT_PREFLIGHT = 2


@dataclass
class ScoredItem:
    """One task run in one arm, and what the scorer made of it.

    The constructor refuses the two shapes that carried the old bug: an unscored item holding
    a number, and a scored item holding none.
    """

    task_id: str
    arm: str
    run_index: int
    scored: bool
    score: Optional[int]
    reasoning: str = ""
    scoring_mode: str = "judge"
    scorer_model: Optional[str] = None
    input_tokens: Optional[int] = None
    output_tokens: Optional[int] = None

    def __post_init__(self):
        if not self.scored:
            if self.score is not None:
                raise ValueError(
                    f"{self.task_id}/{self.arm}: an unscored item carries score "
                    f"{self.score!r}. A scorer that did not answer has not measured "
                    "anything — score must be None."
                )
            return
        if self.score is None:
            raise ValueError(
                f"{self.task_id}/{self.arm}: scored is True but score is None"
            )
        if isinstance(self.score, bool) or not isinstance(self.score, int):
            raise ValueError(
                f"{self.task_id}/{self.arm}: score {self.score!r} is not an integer on "
                f"the {VALID_SCORES[0]}-{VALID_SCORES[-1]} scale"
            )
        if self.score not in VALID_SCORES:
            raise ValueError(
                f"{self.task_id}/{self.arm}: score {self.score!r} is off the "
                f"{VALID_SCORES[0]}-{VALID_SCORES[-1]} scale"
            )

    @classmethod
    def from_verdict(
        cls, verdict: dict, task_id: str, arm: str, run_index: int
    ) -> "ScoredItem":
        """Build from a `score_result` payload. `scored` is required, not defaulted.

        Defaulting it to True would let an old `{"score": 0}` payload from an unpatched
        caller back in as a measured zero, which is the whole bug wearing a different hat.
        """
        return cls(
            task_id=task_id,
            arm=arm,
            run_index=run_index,
            scored=verdict["scored"],
            score=verdict.get("score"),
            reasoning=verdict.get("reasoning", ""),
            scoring_mode=verdict.get("scoring_mode", "judge"),
            scorer_model=verdict.get("scorer_model"),
            input_tokens=verdict.get("input_tokens"),
            output_tokens=verdict.get("output_tokens"),
        )

    @property
    def passed(self) -> bool:
        return bool(
            self.scored and self.score is not None and self.score >= PASS_THRESHOLD
        )

    def to_dict(self) -> dict:
        return {
            "task_id": self.task_id,
            "arm": self.arm,
            "run_index": self.run_index,
            "scored": self.scored,
            "score": self.score,
            "reasoning": self.reasoning,
            "scoring_mode": self.scoring_mode,
            "scorer_model": self.scorer_model,
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
        }


Item = Union[ScoredItem, dict]


def _is_scored(item: Item) -> bool:
    if isinstance(item, ScoredItem):
        return item.scored
    # A dict without `scored` is a payload from before this module existed. Treating it as
    # scored is exactly the assumption that produced the fabricated zeros.
    return bool(item["scored"])


def _passed(item: Item) -> bool:
    if isinstance(item, ScoredItem):
        return item.passed
    score = item.get("score")
    return bool(item["scored"] and score is not None and score >= PASS_THRESHOLD)


def compute_pass_rate(items: Iterable[Item]) -> Optional[float]:
    """Passes over *scored* items, or `None` when nothing was scored.

    `None` and `0.0` are different facts and the report prints them differently: `—` against
    `0.00`. Collapsing them is what made a broken credential look like a broken skill.
    """
    items = list(items)
    scored = [i for i in items if _is_scored(i)]
    if not scored:
        return None
    return sum(1 for i in scored if _passed(i)) / len(scored)


def unscored_share(items: Iterable[Item]) -> float:
    """Share of all items the scorer could not score. `0.0` for no items at all."""
    items = list(items)
    if not items:
        return 0.0
    return sum(1 for i in items if not _is_scored(i)) / len(items)


def run_is_valid(items: Iterable[Item]) -> bool:
    """Whether the run measured enough to describe the skill.

    A run that scored nothing is invalid however few items it had: there is no number in it.
    """
    items = list(items)
    if not any(_is_scored(i) for i in items):
        return False
    return unscored_share(items) <= UNSCORED_LIMIT


@dataclass
class ArmResult:
    """The four counts and the rate for one arm, so a report can show its denominator."""

    items_total: int
    items_scored: int
    items_unscored: int
    items_passed: int
    pass_rate: Optional[float]

    @classmethod
    def from_items(cls, items: Iterable[Item]) -> "ArmResult":
        items = list(items)
        scored = [i for i in items if _is_scored(i)]
        return cls(
            items_total=len(items),
            items_scored=len(scored),
            items_unscored=len(items) - len(scored),
            items_passed=sum(1 for i in scored if _passed(i)),
            pass_rate=compute_pass_rate(items),
        )

    def to_dict(self) -> dict:
        return {
            "items_total": self.items_total,
            "items_scored": self.items_scored,
            "items_unscored": self.items_unscored,
            "items_passed": self.items_passed,
            "pass_rate": self.pass_rate,
        }


def baseline_write_allowed(run_valid: bool, update_requested: bool) -> bool:
    """`--update-baseline` is a request, not a permission.

    The 2026-08 baseline holds nine entries of fabricated zeros because a run was asked to
    write them and nothing checked whether it had measured anything first.
    """
    return bool(run_valid and update_requested)


def delta_reportable(run_valid: bool) -> bool:
    """An invalid run has no Δ. Subtracting a real number from an unmeasured one is noise."""
    return bool(run_valid)
