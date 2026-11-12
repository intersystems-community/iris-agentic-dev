"""The pilot run — 121 T029, and the input to T030's go/no-go.

Eight tasks, three arms, one paired comparison per adjacent pair of arms. This is the one deliberate
spend in the feature (~$2 at `openai/gpt-4.1`), and what it buys is a decision: whether the full
corpus is worth building at all. FR-024 makes that decision unpublishable, so nothing here produces a
figure — `purpose="design_decision"` travels with both comparisons and `Comparison.publishable` is
False by construction.

No statistics live in this file. `comparison.pair_arms` pairs on task ID, `Comparison.pilot_verdict`
holds the go/no-go rule, and `stats.mcnemar_test` gives the exact p-value. A second copy of the `b >= 5`
threshold here is a second copy to keep in step, so there isn't one.

What this file does own is the bookkeeping between a session and a discordant count:

- **Every arm is checked both ways before its session starts.** `arms.assert_absent` for what it must
  not have, `arms.assert_present` for what it must (FR-002). The 118 failure was an arm that ran with
  no tools and published the zeros.
- **Every task is re-validated live before every arm.** `graded_task.validate_live` costs no model
  tokens and answers the one question the previous arm's agent could have broken: that the check is
  false against the fixture and true against the reference. PILOT-05 already failed this way once,
  from a global an earlier reference had left behind.
- **A session that could not run scores nothing.** `passed=None`, which drops the pair rather than
  filing a harness fault in the arm's column.

The prompt is identical in all three arms, including the connection details and the container name.
The bare arm keeps `bash`, so it can reach IRIS the hard way if the model thinks to; what it does not
have is the tools. Withholding the connection information as well would measure the prompt.

Run it:

    IAD_BINARY=$PWD/target/debug/iris-agentic-dev \\
    IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \\
    python -m tests.e2e.skill_eval.pilot
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
import time
from dataclasses import asdict, dataclass, field

from tests.e2e import billing
from tests.e2e.skill_eval import graded_task
from tests.e2e.skill_eval.models import localize_prompt
from tests.e2e.skill_eval.arms import ARMS, MCP_SERVER_NAME, Arm, installed_skill_names
from tests.e2e.skill_eval.driver import server_never_answered
from tests.e2e.skill_eval.comparison import Comparison, pair_arms
from tests.e2e.skill_eval.graded_task import CheckBroken, GradedTask

#: FR-024. Set once, here, so no call site can build a pilot comparison as a result.
PILOT_PURPOSE = "design_decision"

#: The pilot's model. Held with the runner rather than passed in, because the arms are only
#: comparable to each other when the model is the same in all three.
PILOT_MODEL = "openai/gpt-4.1"

#: The same clock in every arm. A session killed here counts as a failure, not as a hole — it did not
#: solve the task — but it is recorded as a timeout, because a discordant pair that turns on the clock
#: is a different claim from one that turns on capability.
SESSION_TIMEOUT = 300

SKILLS_PACK_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "skills", "skills")
)

RESULTS_DIR = os.path.join(os.path.dirname(os.path.dirname(__file__)), "results")


def shipped_skills() -> tuple[str, ...]:
    """Every skill in the pack, which is what the skills arm installs.

    The whole pack rather than a per-task selection. No pilot task names a skill
    (`test_no_pilot_task_names_a_skill`), and picking the two or three skills that suit PILOT-03 would
    be answering the task on the arm's behalf — the reach question in Story 4 is precisely whether the
    agent finds the right skill on its own. It is also what a user actually has installed: the plugin
    ships the pack, and opencode discloses names and descriptions rather than bodies, so a wide pack
    costs prompt tokens roughly in proportion to its index.
    """
    if not os.path.isdir(SKILLS_PACK_DIR):
        return ()
    return tuple(
        sorted(
            name
            for name in os.listdir(SKILLS_PACK_DIR)
            if os.path.isfile(os.path.join(SKILLS_PACK_DIR, name, "SKILL.md"))
        )
    )


@dataclass(frozen=True)
class ArmRun:
    """One task in one arm: what the check said, and what to distrust about it."""

    task_id: str
    arm: str
    #: True/False from the check. None means the check never answered, which is not a failure.
    passed: bool | None
    reason: str | None = None
    timed_out: bool = False
    tool_calls: int = 0
    #: The whole run, validation included.
    seconds: float = 0.0
    #: The session alone, which is what the timeout applies to.
    session_seconds: float = 0.0
    #: Which repeat of this task-in-this-arm. The pilot ran each pair once and left this 0; the graded
    #: ladder repeats, and `ladder.arm_verdict` reduces the repeats by a stated rule.
    run_index: int = 0
    #: The call log, as JSON-ready records. `tool_calls` above is its length over completed calls and
    #: cannot answer which tool was reached for, which is the whole of Story 5's join. Recorded here
    #: rather than re-derived later because the event stream exists only inside the session.
    calls: tuple[dict, ...] = ()
    #: Whether the session loaded the arm's skill with the `skill` tool. None for an arm that does not
    #: preload, where the question does not apply.
    skill_loaded: bool | None = None
    #: Input, output, reasoning and cache tokens, summed from the session's `step_finish` events (135).
    #: Empty for a record written before then.
    tokens: dict = field(default_factory=dict)
    #: Dollars the session billed (sum of `step_finish.cost`). None for a record from before 135, which
    #: printed the figure and did not store it.
    cost_usd: float | None = None
    #: How many times the session was run to get this result: more than 1 when the iad server never
    #: answered in an earlier attempt (the attempt that is kept is the last).
    attempts: int = 1
    #: The fraction of the task's `quality_checks` that passed, beside `passed` (135). None when the
    #: task declares none or none answered.
    quality: float | None = None
    quality_detail: dict = field(default_factory=dict)


#: In front of every session, every arm. Before 130 round 3 the harness leaked the operator's own
#: instructions into sessions, and one of them was "execute all steps to completion". With it gone,
#: gpt-4.1 under `opencode run` often ends its turn on a plan ("Next, I'll update the method") and
#: the session is over. The line is the same in every arm, so it moves both sides of a comparison.
AUTONOMY_PREAMBLE = (
    "Work on this task until it is done. Do not stop to describe a plan or to ask for "
    "confirmation; carry out each step yourself.\n\n"
)


def preload_preamble(skill_names: tuple[str, ...]) -> str:
    """What a preloading arm puts in front of the task."""
    names = ", ".join(f"`{n}`" for n in skill_names)
    return (
        f"Before you start, load the {names} skill with your skill tool, read it, and follow it "
        "while you work.\n\n"
    )


def skill_loaded(events: list[dict], skill_names: tuple[str, ...]) -> bool:
    """True if a completed opencode `skill` call named one of `skill_names`."""
    for event in events:
        if event.get("type") != "tool_use":
            continue
        part = event.get("part", {})
        state = part.get("state", {})
        if part.get("tool") != "skill" or state.get("status") != "completed":
            continue
        if (state.get("input") or {}).get("name") in skill_names:
            return True
    return False


def call_records(tool_calls) -> tuple[dict, ...]:
    """The call log in the shape the artifact stores: name, server, status, outcome, error.

    `arguments` is dropped. A single `iris_doc` call carries a whole class definition, the report is
    committed, and a per-tool table needs to know which tool was called and what came back — not the
    payload. If an argument-level finding is ever wanted, the transcript is the place for it.
    """
    return tuple(
        {
            "name": call.name,
            "server": call.server,
            "completed": call.completed,
            "status": call.status,
            "error": call.error,
        }
        for call in tool_calls
    )


def arm_results(runs) -> dict[str, bool]:
    """The scored runs, as `pair_arms` wants them. Unscored runs are absent, not False."""
    return {run.task_id: bool(run.passed) for run in runs if run.passed is not None}


def pilot_comparisons(runs) -> tuple[Comparison, ...]:
    """One comparison per adjacent pair of arms, in arm order.

    An arm with nothing scored raises. Left alone it would pair as an arm that failed every task, and
    the arm above it would collect the credit for a difference that was a harness fault — the exact
    shape of the nine skills that published 0.00.
    """
    by_arm = {
        arm.name: arm_results([r for r in runs if r.arm == arm.name]) for arm in ARMS
    }
    empty = [name for name, results in by_arm.items() if not results]
    if empty:
        raise ValueError(
            f"the {', '.join(empty)} arm produced no scored run at all, so there is nothing to pair. "
            "An arm of holes is not an arm that failed: pairing it would credit the difference to the "
            "arm above it"
        )
    return tuple(
        pair_arms(
            lower.name,
            upper.name,
            by_arm[lower.name],
            by_arm[upper.name],
            purpose=PILOT_PURPOSE,
        )
        for lower, upper in zip(ARMS, ARMS[1:])
    )


# --- one session ---------------------------------------------------------------------------------


def completed_tool_calls(events) -> int:
    """Tool calls the session actually completed. Reported beside the verdict, never scored on."""
    return sum(
        1
        for event in events
        if event.get("type") == "tool_use"
        and event.get("part", {}).get("state", {}).get("status") == "completed"
    )


def hit_the_clock(session_seconds: float, timeout: int) -> bool:
    """Whether the session was killed on the timer rather than finishing.

    The clock, not the event stream. `run_opencode` stops reading when opencode reports the session
    idle, so an absent idle event looks like the timeout tell — but opencode does not reliably emit
    one: the first live pilot session finished in 44 seconds with 7 completed tool calls and a passing
    check, and no `session.status` idle event anywhere in its output. Inferring a timeout from that
    would have labelled every run a timeout. The kill timer fires at exactly `timeout` seconds after
    the spawn, so elapsed session time is the thing that actually knows.
    """
    return session_seconds >= timeout


def to_driver_run(driver, session, *, passed, reason: str | None):
    """The pilot's verdict in the boundary's shape (spec 120 FR-006, FR-010).

    `passed` is a tri-state bool and the boundary's is a scalar reward beside a `scored` flag, so the
    conversion lives in one place. `DriverRun` does the refusing: a run the harness could not grade
    cannot come out of here holding a 0, which is the 118 bug this program exists downstream of.
    """
    if passed is None:
        return driver.grade(session, scored=False, reason=reason)
    return driver.grade(
        session, reward=1.0 if passed else 0.0, scored=True, reason=reason
    )


def default_driver():
    """The driver a session runs under when the caller did not name one — `opencode_driver` owns it.

    Re-exported here because `run_one` is where the default is reached for, and because `ladder.report`
    reads the same function to attribute the run. A default in two places is a provenance field that
    disagrees with the run it describes.
    """
    from tests.e2e.skill_eval.opencode_driver import default_driver as _default

    return _default()


def run_one(
    task: GradedTask,
    arm: Arm,
    *,
    openai_api_key: str,
    model: str = PILOT_MODEL,
    timeout: int = SESSION_TIMEOUT,
    skill_names: tuple[str, ...] | None = None,
    iris_host: str,
    iris_web_port: str,
    iris_container: str,
    binary: str | None = None,
    driver=None,
    on_events=None,
    effort: str | None = None,
) -> ArmRun:
    """One task, one arm, one session, one check.

    `on_events` gets the session's raw event stream as soon as it exists, before grading, so a
    transcript survives a check that raises.

    Order is the whole of the method. Validate live (free), put the fixture back, assert the arm both
    ways, run the session, read the check. A step that raises makes the run unscored rather than
    failed, because every one of them is the harness's problem and not the agent's.
    """
    from tests.e2e.isolated_env import IsolatedEnv
    from tests.e2e.skill_eval.driver import completed_calls

    driver = driver or default_driver()
    started = time.monotonic()
    fixture_names = [doc.name for doc in task.fixtures]

    try:
        # Free, and the only thing that can catch the previous arm's agent having made the check
        # trivially true or impossible. Leaves the reference in place, hence the reset below.
        graded_task.validate_live(task)
        graded_task.reset_documents(fixture_names, task.namespace)
        graded_task.apply_documents(task.fixtures, task.namespace)
        # FR-023. What is here now is the fixture and whatever was here before; anything new after
        # the check is this session's, and it goes. `Bench.Q2.CountOther` sat in BENCHMARK for two
        # days and ended two SKILL-09 sessions at 3 calls because nothing deleted it.
        prefixes = graded_task.package_prefixes(task.fixtures)
        snapshot = graded_task.list_classes(prefixes, task.namespace)
        # 132: the reference left ConfigStore, Wallet or web-app state behind that no class reset
        # removes; the session must not start on it.
        graded_task.run_teardown(task)
        # Spec 172: the globals the task's classes read, after the teardown cleared what was there.
        graded_task.run_seed(task)
    except (CheckBroken, graded_task.CorpusInvalid) as exc:
        return ArmRun(
            task_id=task.id,
            arm=arm.name,
            passed=None,
            reason=f"the task did not validate against IRIS before the session: {exc}",
            seconds=time.monotonic() - started,
        )

    with IsolatedEnv(openai_api_key=openai_api_key, model=model, effort=effort) as env:
        # The driver configures its own arm and returns the environment its own session needs. Under
        # opencode that is `arms.configure` plus `OPENCODE_CONFIG_CONTENT`; under prime-agent it is a
        # sandboxed HOME holding a settings file. Reading one harness's variables here is what made
        # the `driver` parameter decorative.
        env_vars = driver.configure_arm(
            arm,
            env,
            shipped_skills() if skill_names is None else skill_names,
            iris_host=iris_host,
            iris_web_port=iris_web_port,
            iris_container=iris_container,
            binary=binary,
            openai_api_key=openai_api_key,
        )
        with tempfile.TemporaryDirectory(prefix=f"pilot-{task.id}-") as cwd:
            # FR-002, both directions, on the environment the session is actually about to get.
            arms_check = _assert_arm(arm, env_vars, env.skills_dir, cwd, driver=driver)
            if arms_check is not None:
                return ArmRun(
                    task_id=task.id,
                    arm=arm.name,
                    passed=None,
                    reason=arms_check,
                    seconds=time.monotonic() - started,
                )
            session_started = time.monotonic()
            # Through the boundary, not around it. `session_from_events` keeps the server that served
            # each call and the calls that errored; `completed_calls` over that log is the same number
            # `completed_tool_calls` read off the raw stream, which `test_opencode_driver.py` pins
            # because `pilot-121.json` is the only graded evidence this program has.
            try:
                prompt = (
                    AUTONOMY_PREAMBLE
                    + (preload_preamble(arm.skill_names) if arm.preload else "")
                    + localize_prompt(
                        task.prompt, web_port=iris_web_port, container=iris_container
                    )
                )
                events = driver.collect_events(
                    prompt,
                    env_vars,
                    model=model,
                    timeout=timeout,
                    working_dir=cwd,
                )
            except Exception as exc:  # the harness, not the agent
                # A session that never started is a hole in the arm. Left to fall through it would
                # reach the check, the check would say no, and the arm would publish a FAIL that
                # describes the daemon rather than the model. prime-agent's supervisor can die before
                # the model is ever reached, and the Phase 2 gate did exactly that three times.
                return ArmRun(
                    task_id=task.id,
                    arm=arm.name,
                    passed=None,
                    reason=f"the {getattr(driver, 'name', 'agent')} session did not run: {exc}",
                    seconds=time.monotonic() - started,
                    session_seconds=time.monotonic() - session_started,
                )
            if on_events is not None:
                on_events(events)
            session = driver.session_from_events(
                events, session_seconds=time.monotonic() - session_started
            )
            session_seconds = session.session_seconds
            loaded = skill_loaded(events, arm.skill_names) if arm.preload else None

    try:
        passed = graded_task.run_check(task)
        reason = None
    except CheckBroken as exc:
        passed, reason = None, f"the check did not answer: {exc}"
    # Read before the cleanup below, while IRIS still holds what the session left.
    quality_detail = graded_task.run_quality(task) if task.quality_checks else {}
    try:
        graded_task.reset_documents(
            graded_task.created_classes(
                snapshot, graded_task.list_classes(prefixes, task.namespace)
            ),
            task.namespace,
        )
    except CheckBroken as exc:
        # The verdict stands; the next session's own snapshot will not see this one's leftovers,
        # so say so where the operator is watching.
        print(
            f"{task.id} {arm.name}: session leftovers not cleaned: {exc}",
            file=sys.stderr,
        )
    try:
        graded_task.run_teardown(task)
    except CheckBroken as exc:
        # Unlike a class leftover, this state is invisible to the next snapshot and would decide the
        # next session's check, so the run is not scored.
        passed, reason = None, str(exc)
    if passed is not None and server_never_answered(session.tool_calls, MCP_SERVER_NAME):
        # The check still ran, but the arm never had its tools: the answer is the server's.
        passed, reason = (
            None,
            "the iad server never answered (every call timed out), so the session ran without its tools",
        )
    if loaded is False:
        # The check still ran, so IRIS is left as the agent left it, but the answer is not this
        # arm's: a skill the agent never read cannot be credited or blamed.
        passed, reason = (
            None,
            (
                f"the skill arm never loaded {', '.join(arm.skill_names)}, so the run says nothing "
                "about the skill"
            ),
        )

    # Constructed for its refusals, and for the harness swap: everything above this line is what a
    # `prime-agent` driver would hand back unchanged.
    graded = to_driver_run(driver, session, passed=passed, reason=reason)

    return ArmRun(
        task_id=task.id,
        arm=arm.name,
        passed=passed,
        reason=reason,
        timed_out=hit_the_clock(session_seconds, timeout),
        tool_calls=completed_calls(graded.tool_calls),
        calls=call_records(graded.tool_calls),
        seconds=time.monotonic() - started,
        session_seconds=session_seconds,
        skill_loaded=loaded,
        quality=graded_task.quality_score(quality_detail),
        quality_detail=quality_detail,
    )


def _assert_arm(
    arm: Arm, env_vars: dict, skills_dir: str, cwd: str, driver=None
) -> str | None:
    """FR-002 in both directions, as a reason string rather than an exception.

    A contaminated arm stops this run and not the pilot: the other twenty-three sessions are still
    worth having, and the hole is named in the output either way.
    """
    from tests.e2e.skill_eval.arms import ArmContaminated, assert_absent, assert_present

    project_files = [os.path.join(cwd, name) for name in ("CLAUDE.md", "AGENTS.md")]
    try:
        assert_absent(
            arm,
            env_vars,
            skills_dir=skills_dir,
            project_files=project_files,
            driver=driver,
        )
        assert_present(arm, env_vars, skills_dir=skills_dir, driver=driver)
    except ArmContaminated as exc:
        return str(exc)
    return None


# --- the run -------------------------------------------------------------------------------------


def run_pilot(
    tasks=None,
    arms=ARMS,
    *,
    openai_api_key: str | None = None,
    model: str = PILOT_MODEL,
    timeout: int = SESSION_TIMEOUT,
    skill_names: tuple[str, ...] | None = None,
    iris_host: str | None = None,
    iris_web_port: str | None = None,
    iris_container: str = "iris-dev-iris",
    binary: str | None = None,
    driver=None,
) -> list[ArmRun]:
    """Every task in every arm, one session at a time.

    Serial on purpose. Every session shares one BENCHMARK namespace, so two at once would grade each
    other's work — the Python equivalent of the `--test-threads=1` rule the Rust IRIS tests run under.

    Task-major rather than arm-major: a task runs in all three arms before the next task starts, so an
    interrupted pilot leaves whole comparable pairs behind instead of one finished arm.
    """
    key = openai_api_key or os.environ.get("OPENAI_API_KEY")
    if not key:
        raise RuntimeError(
            "OPENAI_API_KEY is not set, so every session would fail to start and every arm would "
            "score as an arm that failed every task"
        )
    tasks = tuple(tasks) if tasks is not None else graded_task.pilot_tasks()
    host = iris_host or os.environ.get("IRIS_HOST", "localhost")
    port = iris_web_port or os.environ.get("IRIS_WEB_PORT", "52780")

    runs: list[ArmRun] = []
    for task in tasks:
        for arm in arms:
            run = run_one(
                task,
                arm,
                openai_api_key=key,
                model=model,
                timeout=timeout,
                skill_names=skill_names,
                iris_host=host,
                iris_web_port=port,
                iris_container=iris_container,
                binary=binary,
                driver=driver,
            )
            runs.append(run)
            print(
                f"{run.task_id:<10} {run.arm:<13} "
                f"{'—' if run.passed is None else ('PASS' if run.passed else 'FAIL'):<5} "
                f"{run.tool_calls:>3} tool calls  {run.seconds:6.1f}s"
                + ("  TIMEOUT" if run.timed_out else "")
                + (f"  [{run.reason}]" if run.reason else ""),
                flush=True,
            )
    return runs


def report(runs, comparisons, arms=ARMS, skill_names=None) -> dict:
    """The whole pilot in one shape, including what it is not allowed to claim.

    `skills_installed` is keyed by arm and derived from the arms themselves. It used to be
    `list(shipped_skills())` computed without looking at anything, which is fine for exactly one
    ladder — the three-arm pilot with the whole pack — and wrong for the one Goal 2 runs: a per-skill
    rung holding `objectscript-list-patterns` would have been recorded as all 34, and the artifact
    naming the intervention would name the wrong intervention.

    `pilot-121.json` predates this and holds the flat list. It is the record of a run that happened and
    is not rewritten; `test_pilot.py` reads it under its own shape.
    """
    installed = {
        arm.name: list(
            installed_skill_names(arm, tuple(skill_names or shipped_skills()))
        )
        for arm in arms
        if arm.skills
    }
    return {
        "purpose": PILOT_PURPOSE,
        "publishable": False,
        "note": (
            "FR-024: a design decision about whether to build the corpus, not a measurement of the "
            "tools. The counts and the one-sided exact p-value are the whole of the claim."
        ),
        "model": PILOT_MODEL,
        "timeout_seconds": SESSION_TIMEOUT,
        "arms": [arm.name for arm in arms],
        "skills_installed": installed,
        "runs": [asdict(run) for run in runs],
        "comparisons": [
            {**comparison.to_dict(), "pilot_verdict": comparison.pilot_verdict}
            for comparison in comparisons
        ],
    }


def _main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--model", default=PILOT_MODEL)
    parser.add_argument("--timeout", type=int, default=SESSION_TIMEOUT)
    parser.add_argument(
        "--task",
        action="append",
        default=None,
        help="run only these task ids (repeatable); default is the whole pilot corpus",
    )
    parser.add_argument("--container", default="iris-dev-iris")
    parser.add_argument("--out", default=None)
    args = parser.parse_args(argv)

    tasks = graded_task.pilot_tasks()
    if args.task:
        wanted = set(args.task)
        tasks = tuple(task for task in tasks if task.id in wanted)
        if not tasks:
            parser.error(f"no pilot task matches {sorted(wanted)}")

    runs = run_pilot(
        tasks,
        model=args.model,
        timeout=args.timeout,
        iris_container=args.container,
        binary=os.environ.get("IAD_BINARY"),
    )
    comparisons = pilot_comparisons(runs)

    for comparison in comparisons:
        print(comparison.summary())
        print(f"  pilot verdict: {comparison.pilot_verdict}")

    os.makedirs(RESULTS_DIR, exist_ok=True)
    out = args.out or os.path.join(
        RESULTS_DIR, f"pilot-{time.strftime('%Y%m%dT%H%M%S')}.json"
    )
    with open(out, "w", encoding="utf-8") as handle:
        json.dump(report(runs, comparisons), handle, indent=2)
    print(f"written to {out}")
    return 0


def main(argv=None) -> int:
    """The pilot, with billable sessions permitted for the duration — see `tests/e2e/billing.py`."""
    with billing.allow():
        return _main(argv)


if __name__ == "__main__":
    raise SystemExit(main())
