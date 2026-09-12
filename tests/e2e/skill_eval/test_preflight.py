"""Unit tests for the preflight — 118 T010, plus the live scorer probe of T012.

The preflight exists because the 2026-08 nightly spent about $3.70 and two hours a night
running nine skills whose scorer it could not reach, and reported the result as a 0% pass
rate. Everything below is one question in different clothes: does the run find out before
it spends anything?

The scorer is stubbed with a recording double rather than mocked loosely, because two of
these tests are about *how many* calls happen. A retry loop in the probe is the difference
between one cent and nine on a broken credential.
"""

import argparse
import json
import os
from unittest.mock import MagicMock

import pytest

from tests.e2e.skill_eval import preflight


def args(**overrides):
    base = {
        "list_skills": False,
        "dry_run": False,
        "merge_results": None,
        "skill": None,
        "category": None,
    }
    base.update(overrides)
    return argparse.Namespace(**base)


class RecordingScorer:
    """A client that records every scoring call, so "exactly one" is checkable."""

    def __init__(
        self,
        text='{"score": 3, "reasoning": "probe"}',
        model="claude-sonnet-4-6",
        fail=None,
    ):
        self.calls: list[dict] = []
        self._text = text
        self._model = model
        self._fail = fail
        self.messages = MagicMock()
        self.messages.create = self._create

    def _create(self, **kwargs):
        self.calls.append(kwargs)
        if self._fail is not None:
            raise self._fail
        msg = MagicMock()
        msg.content = [MagicMock(text=self._text)]
        msg.model = self._model
        msg.usage = MagicMock(input_tokens=42, output_tokens=11)
        return msg


@pytest.fixture
def stub_scorer(monkeypatch):
    """A reachable scorer. Returned so a test can count its calls."""
    client = RecordingScorer()
    monkeypatch.setattr(preflight, "make_client", lambda: client)
    return client


@pytest.fixture
def stub_binary(tmp_path, monkeypatch):
    """A resolvable binary with a listable tool surface."""
    monkeypatch.setattr(preflight.provenance, "resolve_binary", lambda: "/tmp/iad-stub")
    monkeypatch.setattr(
        preflight.provenance, "tool_surface", lambda b: "1.4.1+abcdef123456"
    )
    return "/tmp/iad-stub"


@pytest.fixture
def good_corpus(monkeypatch):
    monkeypatch.setattr(preflight, "validate_corpus", lambda skills=None: None)


# ---------------------------------------------------------------------------
# The happy path, so the failures below mean something
# ---------------------------------------------------------------------------


def test_a_ready_run_passes_and_reports_what_it_resolved(
    stub_scorer, stub_binary, good_corpus
):
    result = preflight.preflight(args())
    assert result.ok is True
    assert result.scorer_model == "claude-sonnet-4-6"
    assert result.binary == "/tmp/iad-stub"
    assert result.tool_surface == "1.4.1+abcdef123456"
    assert result.failure is None


def test_the_probe_makes_exactly_one_scoring_call(
    stub_scorer, stub_binary, good_corpus
):
    """One call, not a retry loop. A broken credential should cost one cent, not nine."""
    preflight.preflight(args())
    assert len(stub_scorer.calls) == 1, stub_scorer.calls


def test_the_probe_asks_for_almost_no_output(stub_scorer, stub_binary, good_corpus):
    """The probe is a reachability check. Paying for a full rubric answer is waste."""
    preflight.preflight(args())
    assert stub_scorer.calls[0]["max_tokens"] <= 64


# ---------------------------------------------------------------------------
# Skips
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    "override",
    [{"list_skills": True}, {"dry_run": True}, {"merge_results": "results/"}],
    ids=["list-skills", "dry-run", "merge-results"],
)
def test_modes_that_spend_nothing_skip_the_preflight(override, monkeypatch):
    """These three never start a session, so requiring a credential would be theatre.

    `--list-skills` in particular builds the CI matrix, and a matrix job that needs the
    scorer secret to enumerate directory names would fail the whole nightly on a rotation.
    """
    called = []
    monkeypatch.setattr(preflight, "make_client", lambda: called.append("scorer"))
    result = preflight.preflight(args(**override))
    assert result.ok is True
    assert result.skipped is True
    assert not called, "the preflight called the scorer in a mode that spends nothing"


# ---------------------------------------------------------------------------
# Failures
# ---------------------------------------------------------------------------


def test_an_unreachable_scorer_names_what_was_looked_for(
    monkeypatch, stub_binary, good_corpus
):
    monkeypatch.setattr(
        preflight,
        "make_client",
        lambda: RecordingScorer(fail=RuntimeError("no credentials")),
    )
    for var in preflight.CREDENTIAL_VARS:
        monkeypatch.delenv(var, raising=False)
    monkeypatch.setenv("OPENAI_API_KEY", "sk-present-but-not-the-scorer")

    result = preflight.preflight(args())
    assert result.ok is False
    assert result.stage == "scorer"
    for var in preflight.CREDENTIAL_VARS:
        assert var in result.failure, f"{var} missing from the message"
    assert "OPENAI_API_KEY" in result.failure, (
        "a present-but-unused key is the likely confusion"
    )
    assert "nothing was spent" in result.failure


def test_a_scorer_that_answers_unparseably_fails_the_preflight(
    monkeypatch, stub_binary, good_corpus
):
    """Reachable is not the same as usable. A model that will not emit JSON scores nothing."""
    monkeypatch.setattr(
        preflight, "make_client", lambda: RecordingScorer(text="I would rather not.")
    )
    result = preflight.preflight(args())
    assert result.ok is False
    assert result.stage == "scorer"


def test_a_missing_binary_names_the_three_places(monkeypatch, stub_scorer, good_corpus):
    monkeypatch.setattr(preflight.provenance, "resolve_binary", lambda: None)
    result = preflight.preflight(args())
    assert result.ok is False
    assert result.stage == "binary"
    assert "IAD_BINARY" in result.failure and "PATH" in result.failure


def test_a_toolless_binary_fails_the_preflight(monkeypatch, stub_scorer, good_corpus):
    """`tool_surface == "none"` means the sessions would run without iad tools.

    That is the exact state of the runs this spec was written about, and it needs no IRIS to
    detect: `tool --list` reads the router.
    """
    monkeypatch.setattr(preflight.provenance, "resolve_binary", lambda: "/tmp/iad-stub")
    monkeypatch.setattr(preflight.provenance, "tool_surface", lambda b: "none")
    result = preflight.preflight(args())
    assert result.ok is False
    assert result.stage == "tool_surface"


def test_a_broken_corpus_is_reported_before_the_credential(monkeypatch, stub_binary):
    """Cheapest first, and it is part of the contract.

    A run with both problems must report the corpus: fixing the credential would not have
    helped, and a message that names the wrong cause sends someone to rotate a secret for
    an afternoon.
    """
    monkeypatch.setattr(
        preflight,
        "validate_corpus",
        lambda skills=None: (
            "IRIS-A: tasks/skills/targeted/IRIS-A.yaml is not valid YAML"
        ),
    )
    monkeypatch.setattr(
        preflight,
        "make_client",
        lambda: RecordingScorer(fail=RuntimeError("no credentials")),
    )

    result = preflight.preflight(args())
    assert result.ok is False
    assert result.stage == "corpus"
    assert "IRIS-A.yaml" in result.failure
    assert "AWS_BEARER_TOKEN_BEDROCK" not in result.failure


def test_a_broken_corpus_spends_nothing(monkeypatch, stub_binary):
    scorer = RecordingScorer()
    monkeypatch.setattr(preflight, "make_client", lambda: scorer)
    monkeypatch.setattr(
        preflight, "validate_corpus", lambda skills=None: "IRIS-A: missing task file"
    )
    preflight.preflight(args())
    assert not scorer.calls


# ---------------------------------------------------------------------------
# Order
# ---------------------------------------------------------------------------


def test_the_checks_run_cheapest_first(monkeypatch, stub_scorer):
    order = []

    def corpus(skills=None):
        order.append("corpus")
        return None

    def resolve():
        order.append("binary")
        return "/tmp/iad-stub"

    def surface(binary):
        order.append("tool_surface")
        return "1.4.1+abcdef123456"

    original_create = stub_scorer.messages.create

    def create(**kwargs):
        order.append("scorer")
        return original_create(**kwargs)

    stub_scorer.messages.create = create
    monkeypatch.setattr(preflight, "validate_corpus", corpus)
    monkeypatch.setattr(preflight.provenance, "resolve_binary", resolve)
    monkeypatch.setattr(preflight.provenance, "tool_surface", surface)

    preflight.preflight(args())
    assert order == ["corpus", "scorer", "binary", "tool_surface"]


# ---------------------------------------------------------------------------
# Corpus validation, against the real corpus
# ---------------------------------------------------------------------------


def test_validate_corpus_passes_on_the_committed_corpus():
    """The corpus in the tree is valid, so a failure here is a real broken task file."""
    assert preflight.validate_corpus() is None


def test_validate_corpus_names_the_task_and_the_file(tmp_path, monkeypatch):
    bad = tmp_path / "IRIS-BROKEN.yaml"
    bad.write_text("description: [unclosed\n")
    monkeypatch.setattr(preflight, "_TARGETED_DIR", str(tmp_path))
    monkeypatch.setattr(
        preflight,
        "_eval_configs",
        lambda skills=None: [("iris-broken", {"benchmark_tasks": ["IRIS-BROKEN"]})],
    )
    failure = preflight.validate_corpus()
    assert failure and "IRIS-BROKEN" in failure and str(bad) in failure


def test_validate_corpus_reports_a_task_that_does_not_exist(monkeypatch):
    monkeypatch.setattr(
        preflight,
        "_eval_configs",
        lambda skills=None: [("iris-ghost", {"benchmark_tasks": ["NO-SUCH-TASK"]})],
    )
    failure = preflight.validate_corpus()
    assert failure and "NO-SUCH-TASK" in failure


# ---------------------------------------------------------------------------
# T021 — `--preflight-only`, the CI gate that runs before the matrix
# ---------------------------------------------------------------------------


def _run_cli(*cli_args, env=None):
    import subprocess
    import sys

    repo_root = os.path.abspath(
        os.path.join(os.path.dirname(__file__), "..", "..", "..")
    )
    environ = dict(os.environ)
    environ["PYTHONPATH"] = repo_root
    for var in preflight.CREDENTIAL_VARS:
        environ.pop(var, None)
    environ.pop("CLAUDE_CODE_USE_BEDROCK", None)
    environ.update(env or {})
    return subprocess.run(
        [sys.executable, "-m", "tests.e2e.skill_eval", *cli_args],
        cwd=repo_root,
        env=environ,
        capture_output=True,
        text=True,
        timeout=180,
    )


def test_preflight_only_with_no_credential_exits_2_and_starts_no_session():
    """The gate the nightly needed. One job answers for all nine shards.

    Nine shards each discovering the same missing credential is nine container starts and nine
    `npm install -g opencode` before anything reports the one fact that mattered.
    """
    proc = _run_cli("--preflight-only", "--skill", "iris-connectivity")
    assert proc.returncode == 2, proc.stdout + proc.stderr
    assert "preflight" in (proc.stdout + proc.stderr).lower()


def test_preflight_only_never_reaches_the_eval():
    """`--preflight-only` must not fall through into the run when the checks pass."""
    proc = _run_cli("--preflight-only", "--skill", "iris-connectivity")
    combined = proc.stdout + proc.stderr
    assert "Running skill evaluation" not in combined
    assert "measuring fire-rate" not in combined


# ---------------------------------------------------------------------------
# T012 — the live scorer. Costs one call, so it is opt-in.
# ---------------------------------------------------------------------------


@pytest.mark.skipif(
    os.environ.get("IAD_EVAL_LIVE_SCORER") != "1",
    reason=(
        "makes one real Bedrock call — set IAD_EVAL_LIVE_SCORER=1 to run. Skipped, NOT "
        "passed: no test above has talked to a real scorer, and the failure this spec fixes "
        "was invisible to every stubbed test in the suite."
    ),
)
def test_the_live_scorer_answers_with_a_model_and_a_usage_record():
    """One real call. Asserts the three things the run's provenance and cost record need.

    A stub cannot check this: `msg.model` and `msg.usage` come from the service, and the
    whole reason the report used to print `openai/gpt-4.1` for a Bedrock Sonnet call is that
    nobody had ever read the response's own answer to "who scored this".
    """
    probe = preflight.probe_scorer()
    assert probe.ok, probe.detail
    assert isinstance(probe.model, str) and probe.model
    assert probe.input_tokens and probe.input_tokens > 0
    assert probe.output_tokens and probe.output_tokens > 0
    assert json.loads(probe.raw_text)["score"] in (0, 1, 2, 3)
