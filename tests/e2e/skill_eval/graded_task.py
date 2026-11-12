"""A graded task, and the validation that decides whether it is allowed to produce a number.

121 T026/T028. FR-003, FR-004 and FR-022 in code.

A task is a YAML file with four parts and no fifth:

```yaml
id: PILOT-01
prompt: |            what the agent is asked
fixtures:            classes written to the benchmark namespace before the session
  - name: Pilot.X
    content: |
      Class Pilot.X ...
check: |             ObjectScript that prints exactly PASS or exactly FAIL
  write $select(<assertion>:"PASS",1:"FAIL")
solution:            the reference: the same classes, correct
  - name: Pilot.X
    content: |
      ...
```

There is deliberately no rubric, no `expected_behavior`, and no judge. `benchmark/021`'s tasks have
all three, and the two defects that produced four skills at 0.00 against 0.00 both lived in the
distance between what an agent did and what a judge was shown. A check that prints PASS from an
ObjectScript assertion has no such distance.

Validation is two-stage, because the two halves can answer different amounts on their own:

- `validate_shape` — pure, runs in the unit suite on every commit. Fields present, no judge fields,
  the reference solution addresses the fixture's own classes, the check reads no clock and rolls no
  dice.
- `validate_live` — needs IRIS, which is the only thing that can answer FR-004 and FR-022: apply the
  fixture and the check must FAIL, apply the reference and the same check must PASS. Costs no model
  tokens, so it runs before every pilot session rather than once.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
from dataclasses import dataclass, field

import yaml

BENCHMARK_NAMESPACE = "BENCHMARK"

BENCHMARK_DIR = os.path.join(
    os.path.dirname(os.path.dirname(__file__)), "tasks", "benchmark"
)

#: The eight tasks that decided the go/no-go. `tests/e2e/results/pilot-121.json` names them, so they
#: stay where they are: a task that changes directory stops matching its own recorded result.
PILOT_DIR = os.path.join(BENCHMARK_DIR, "pilot")

#: The tools ladder — bare against tools against tools+skills. 121 T031 grows this one.
CORPUS_DIR = os.path.join(BENCHMARK_DIR, "corpus")

#: The skills ladder. One named skill per task, and the discriminator is a documented convention the
#: model cannot infer from the prompt.
SKILL_TASK_DIR = os.path.join(BENCHMARK_DIR, "skills")

#: Skill tasks authored for 135, one subdirectory per skill: `skills/<skill>/<TASK>.yaml`. The directory
#: name is the skill the task measures, and the loader refuses a task whose `skill:` disagrees.
AUTHORED_DIR = os.path.normpath(
    os.path.join(os.path.dirname(__file__), "..", "tasks", "skills")
)

#: Everything the split has to cover. Not a glob over `BENCHMARK_DIR`: a directory added by accident
#: should not silently join the corpus, and the split file itself lives there.
TASK_DIRS = (PILOT_DIR, CORPUS_DIR, SKILL_TASK_DIR)

# Fields whose presence means a human wrote a grading criterion for a model to apply. Rejected by
# name rather than by inspection, because the reason to reject them is what they are for.
JUDGE_FIELDS = (
    "expected_behavior",
    "rubric",
    "judge_model",
    "judge",
    "criteria",
    "score_guidance",
)

# Constructs that make the same check answer differently on a second run: the clock, the dice, the
# process. A figure that moves with the date cannot be compared to last month's figure.
NONDETERMINISTIC = (
    "$H",
    "$HOROLOG",
    "$ZTS",
    "$NOW",
    "$ZH",
    "$ZHOROLOG",
    "$RANDOM",
    "$R(",
    "$JOB",
    "$J,",
)

#: A graded task file is `<PREFIX>-<number>.yaml`. `eval.yaml` beside them is the older judged corpus.
TASK_FILE = re.compile(r"^[A-Z]+-\d+\.yaml$")

PASS_WORD = "PASS"
FAIL_WORD = "FAIL"


class CorpusInvalid(ValueError):
    """A task is not allowed in the gated corpus. Validation fails; the run does not start."""


class CheckBroken(RuntimeError):
    """A check produced output that is neither PASS nor FAIL, so it has said nothing.

    Not a failure. Scoring unreadable check output 0 is the same mistake as scoring a killed session
    0 — it files a harness fault in the skill's column. See `lift.session_evidence_gap`.
    """


@dataclass(frozen=True)
class Document:
    name: str
    content: str


@dataclass(frozen=True)
class QualityCheck:
    """An extra check scored beside pass/fail (135): no collateral damage, other methods unchanged, a
    clean compile, idiomatic status handling. Declared in the task file before the first session."""

    name: str
    check: str


@dataclass(frozen=True)
class GradedTask:
    id: str
    prompt: str
    check: str
    fixtures: tuple[Document, ...]
    solution: tuple[Document, ...]
    namespace: str = BENCHMARK_NAMESPACE
    skill: str | None = None
    tags: tuple[str, ...] = field(default_factory=tuple)
    # ObjectScript that removes state no class cleanup reaches: ConfigStore entries, Wallet secrets, a
    # %SYS web app (132). Run before validation, before the session and after the check.
    teardown: str | None = None
    # ObjectScript that puts in place the globals the task's classes read (spec 172 T112): run after
    # the fixtures, before validation and before the session, and after the teardown.
    seed: str | None = None
    # Scored as a fraction beside the main check, never in place of it.
    quality_checks: tuple[QualityCheck, ...] = field(default_factory=tuple)


def task_fingerprint(task: GradedTask) -> str:
    """A hash of everything about a task that decides its outcome. A run records it, so a task edited
    between two runs cannot be pooled as if it were the same task (135)."""
    body = json.dumps(
        {
            "prompt": task.prompt,
            "check": task.check,
            "fixtures": [[d.name, d.content] for d in task.fixtures],
            "solution": [[d.name, d.content] for d in task.solution],
            "quality": [[q.name, q.check] for q in task.quality_checks],
            "namespace": task.namespace,
            "skill": task.skill,
            "teardown": task.teardown,
            # Only when present, so a task without one keeps the fingerprint it had before.
            **({"seed": task.seed} if task.seed else {}),
        },
        sort_keys=True,
    )
    return hashlib.sha256(body.encode("utf-8")).hexdigest()


def check_verdict(output: str) -> bool:
    """Read a check's stdout. PASS or FAIL, or `CheckBroken` — never a silent third answer."""
    text = output.strip()
    has_pass = PASS_WORD in text
    has_fail = FAIL_WORD in text
    if has_pass and not has_fail:
        return True
    if has_fail and not has_pass:
        return False
    raise CheckBroken(
        f"check output is neither PASS nor FAIL, so it graded nothing: {text!r} — this is a broken "
        "check, not a failed task"
    )


def load_task(path: str) -> GradedTask:
    """Parse one task file and validate its shape. A file that cannot produce a valid task raises."""
    with open(path, encoding="utf-8") as handle:
        raw = yaml.safe_load(handle) or {}
    if not isinstance(raw, dict):
        raise CorpusInvalid(f"{path}: a task is a mapping, got {type(raw).__name__}")

    stem = os.path.splitext(os.path.basename(path))[0]
    declared = raw.get("id")
    if declared != stem:
        raise CorpusInvalid(
            f"{path}: id {declared!r} does not match the filename {stem!r} — a task loaded under one "
            "id and reported under another mislabels every row it appears in"
        )

    present_judge = [name for name in JUDGE_FIELDS if name in raw]
    if present_judge:
        raise CorpusInvalid(
            f"{stem}: FR-003 — {', '.join(present_judge)} is a field a model judge reads. Every "
            "gated task is graded by a deterministic check against IRIS state; there is nowhere for "
            "a judgement to enter"
        )

    for required in ("prompt", "check"):
        if not str(raw.get(required) or "").strip():
            raise CorpusInvalid(f"{stem}: no {required}")

    task = GradedTask(
        id=stem,
        prompt=str(raw["prompt"]),
        check=str(raw["check"]),
        fixtures=_documents(stem, raw.get("fixtures"), "fixtures"),
        solution=_documents(stem, raw.get("solution"), "solution"),
        namespace=str(raw.get("namespace", BENCHMARK_NAMESPACE)),
        skill=raw.get("skill"),
        tags=tuple(raw.get("tags") or ()),
        teardown=str(raw["teardown"]) if str(raw.get("teardown") or "").strip() else None,
        seed=str(raw["seed"]) if str(raw.get("seed") or "").strip() else None,
        quality_checks=_quality_checks(stem, raw.get("quality_checks")),
    )
    validate_shape(task)
    return task


def validate_shape(task: GradedTask) -> None:
    """Everything about a task that can be decided without IRIS."""
    if not task.fixtures:
        raise CorpusInvalid(
            f"{task.id}: FR-004 — no fixture, so there is no untouched state for the check to be "
            "false against. A check that passes before the agent runs measures nothing: every arm "
            "clears it and the lift is 0.00 whatever the tools do"
        )
    if not task.solution:
        raise CorpusInvalid(
            f"{task.id}: FR-022 — no reference solution. A reference is the only thing that tells a "
            "hard task from a broken check, which is the failure that cost this program four skills"
        )
    fixture_names = {doc.name for doc in task.fixtures}
    strangers = sorted(
        doc.name for doc in task.solution if doc.name not in fixture_names
    )
    if strangers:
        raise CorpusInvalid(
            f"{task.id}: the reference solution writes {', '.join(strangers)}, which the fixture "
            f"never set up (fixture: {', '.join(sorted(fixture_names))}). That is a second task in "
            "the same file, not the answer to this one"
        )
    found = _nondeterministic_tokens(task.check)
    if found:
        raise CorpusInvalid(
            f"{task.id}: the check reads {', '.join(found)}, so it can answer differently on a "
            "second run. FR-003's word is deterministic"
        )
    if not _prints_both_words(task.check):
        raise CorpusInvalid(
            f"{task.id}: the check must be able to print both {PASS_WORD} and {FAIL_WORD}; a check "
            "with only one of them cannot discriminate"
        )
    names: set[str] = set()
    for quality in task.quality_checks:
        if quality.name in names:
            raise CorpusInvalid(
                f"{task.id}: quality check {quality.name!r} is declared twice"
            )
        names.add(quality.name)
        found = _nondeterministic_tokens(quality.check)
        if found:
            raise CorpusInvalid(
                f"{task.id}: quality check {quality.name!r} reads {', '.join(found)}, so it can "
                "answer differently on a second run"
            )
        if not _prints_both_words(quality.check):
            raise CorpusInvalid(
                f"{task.id}: quality check {quality.name!r} must be able to print both "
                f"{PASS_WORD} and {FAIL_WORD}"
            )


def _nondeterministic_tokens(check: str) -> list[str]:
    return [token for token in NONDETERMINISTIC if token in check]


def _prints_both_words(check: str) -> bool:
    return PASS_WORD in check and FAIL_WORD in check


def load_dir(path: str) -> tuple[GradedTask, ...]:
    """Every task file in one directory, in id order.

    A directory that does not exist yet is empty rather than an error. `corpus/` and `skills/` fill
    up over two separate goals, and the pilot's own tests should not wait on either.
    """
    if not os.path.isdir(path):
        return ()
    names = sorted(name for name in os.listdir(path) if name.endswith(".yaml"))
    return tuple(load_task(os.path.join(path, name)) for name in names)


def pilot_tasks() -> tuple[GradedTask, ...]:
    """The committed pilot corpus, in id order."""
    return load_dir(PILOT_DIR)


def load_authored(root: str | None = None) -> tuple[GradedTask, ...]:
    """Every task under `skills/<skill>/`, in id order. A missing root is empty.

    The directory is the skill. A task whose `skill:` names another is refused, because a file in one
    skill's directory that measures a different skill puts its sessions in the wrong verdict.
    """
    root = AUTHORED_DIR if root is None else root
    if not os.path.isdir(root):
        return ()
    tasks: list[GradedTask] = []
    for name in sorted(os.listdir(root)):
        path = os.path.join(root, name)
        if os.path.isfile(path):
            if TASK_FILE.match(name):
                raise CorpusInvalid(
                    f"{path}: a task file loose in the authoring root. File it under "
                    "skills/<skill>/, where the directory names the skill it measures"
                )
            continue
        if not os.path.isdir(path) or name.startswith((".", "_")):
            continue
        # The older skill-eval fixtures (`eval.yaml`, `targeted/`) share this tree; only files named
        # like a graded task id are tasks.
        for filename in sorted(os.listdir(path)):
            if not TASK_FILE.match(filename):
                continue
            task = load_task(os.path.join(path, filename))
            if task.skill != name:
                raise CorpusInvalid(
                    f"{task.id} is filed under skills/{name}/ but names skill {task.skill!r}"
                )
            tasks.append(task)
    return tuple(sorted(tasks, key=lambda task: task.id))


def all_tasks() -> tuple[GradedTask, ...]:
    """Every committed task across the three directories and the authored skill tree, in id order.

    A duplicate ID raises. Two tasks under one ID pair against each other across arms, and the lift
    is then computed over a row that is two different tasks — a failure with no symptom.
    """
    tasks: list[GradedTask] = []
    seen: dict[str, str] = {}

    def add(task: GradedTask, source: str) -> None:
        if task.id in seen:
            raise CorpusInvalid(
                f"{task.id} is declared twice: {seen[task.id]} and {source}. One ID is one "
                "task, or the arms pair rows that are not the same task"
            )
        seen[task.id] = source
        tasks.append(task)

    for directory in TASK_DIRS:
        for task in load_dir(directory):
            add(task, directory)
    for task in load_authored():
        add(task, AUTHORED_DIR)
    return tuple(sorted(tasks, key=lambda task: task.id))


def tools_corpus() -> tuple[GradedTask, ...]:
    """The tools ladder: every task that names no skill."""
    return tuple(task for task in all_tasks() if task.skill is None)


def skill_corpus() -> tuple[GradedTask, ...]:
    """The skills ladder: every task that names one.

    The partition is the field and not the directory, so a skill task filed in the wrong place still
    runs in the right ladder rather than quietly inflating the tools number.
    """
    return tuple(task for task in all_tasks() if task.skill is not None)


# --- the live half: IRIS answers FR-004 and FR-022 ----------------------------------------------


def _iad(args: list[str], namespace: str) -> str:
    """One CLI call. Raises `CheckBroken` on a non-zero exit, because a check that could not run has
    not answered — the same rule the killed-session guard applies to sessions."""
    binary = os.environ.get("IAD_BINARY", "iris-agentic-dev")
    command = [binary, args[0], "-n", namespace, *args[1:]]
    result = subprocess.run(command, capture_output=True, text=True, timeout=180)
    if result.returncode != 0:
        raise CheckBroken(
            f"`{' '.join(command[:3])} …` exited {result.returncode}: "
            f"{(result.stderr or result.stdout).strip()[:400]}"
        )
    return result.stdout


def _tool(name: str, payload: dict, namespace: str) -> dict:
    """Dispatch one tool and parse its envelope. A tool the CLI could not run raises."""
    output = _iad(["tool", name, "-a", json.dumps(payload)], namespace)
    try:
        return json.loads(output)
    except json.JSONDecodeError as exc:
        raise CheckBroken(f"{name} returned no JSON: {output.strip()[:300]}") from exc


def apply_documents(documents, namespace: str = BENCHMARK_NAMESPACE) -> None:
    """Write each document, then compile it. The write must succeed; the compile need not.

    Two steps rather than `iris_doc(mode="put", compile=true)`, because a fixture that does not
    compile *is* the task in the debug cases — the one-call form exits non-zero on the compile error
    and the fixture never lands. Here the write is fatal and the compile is data: the check decides.

    Not `$system.OBJ.Compile` from `exec` either. The write gate blocks reaching class code through
    arbitrary execution (`CODE_EDIT_BLOCKED`, matched `$SYSTEM.OBJ.COMPILE`), which is the gate
    working as designed, and is why an earlier attempt compiled nothing while exiting 0.

    `allow_storage_regeneration` is set because a reference solution for a persistent class carries no
    Storage block while the server-side fixture already has a generated one. In a corpus namespace
    that is what is wanted; the confirmation exists to stop it happening to real data.
    """
    for doc in documents:
        _tool(
            "iris_doc",
            {
                "mode": "put",
                "name": f"{doc.name}.cls",
                "content": doc.content,
                "namespace": namespace,
                "allow_storage_regeneration": True,
            },
            namespace,
        )
    for doc in documents:
        try:
            _tool(
                "iris_compile",
                {"target": f"{doc.name}.cls", "namespace": namespace},
                namespace,
            )
        except CheckBroken:
            pass


def run_check(task: GradedTask) -> bool:
    return check_verdict(_iad(["exec", task.check], task.namespace))


def run_quality(task: GradedTask) -> dict[str, bool | None]:
    """Each quality check's verdict, in declaration order. A check that did not answer is None: like a
    killed session, it is a hole and not a fail."""
    results: dict[str, bool | None] = {}
    for quality in task.quality_checks:
        try:
            results[quality.name] = check_verdict(_iad(["exec", quality.check], task.namespace))
        except CheckBroken:
            results[quality.name] = None
    return results


def quality_score(results: dict) -> float | None:
    """The fraction of answered quality checks that passed; None when none answered."""
    answered = [value for value in results.values() if value is not None]
    if not answered:
        return None
    return sum(1 for value in answered if value) / len(answered)


def reset_documents(names, namespace: str = BENCHMARK_NAMESPACE) -> None:
    """Delete the named classes so the next arm starts from the fixture, not from the last arm's
    answer. The comparison is paired per task across arms, so carry-over would report one arm's work
    in the next arm's column.

    Deletion goes through `iris_doc(mode="delete")`, not `$system.OBJ.Delete` in `exec` — the write
    gate blocks that with `CODE_EDIT_BLOCKED`, correctly. A document that was not there is not an
    error here: the first run of a task has nothing to reset.
    """
    for name in sorted(set(names)):
        try:
            _tool(
                "iris_doc",
                {"mode": "delete", "name": f"{name}.cls", "namespace": namespace},
                namespace,
            )
        except CheckBroken:
            pass


def package_prefixes(documents) -> tuple[str, ...]:
    """The top-level packages a task's fixture lives in, which is where a session writes its classes.

    `iris_doc list` refuses a bare wildcard, so the snapshot is per package; a class a session writes
    outside these packages is not seen (130 FR-023).
    """
    return tuple(sorted({doc.name.split(".")[0] for doc in documents}))


def list_classes(prefixes, namespace: str = BENCHMARK_NAMESPACE) -> set[str]:
    """Every class under the given packages, without the `.cls`. A truncated list raises: deleting
    the difference against a partial snapshot would delete classes the session never touched.
    """
    names: set[str] = set()
    for prefix in prefixes:
        listing = _tool(
            "iris_doc",
            {"mode": "list", "category": "CLS", "pattern": f"{prefix}.*"},
            namespace,
        )
        if listing.get("truncated"):
            raise CheckBroken(
                f"iris_doc list {prefix}.* came back truncated, so the snapshot is not whole"
            )
        names.update(
            doc["name"].removesuffix(".cls") for doc in listing.get("documents", [])
        )
    return names


def created_classes(before, after) -> list[str]:
    """The classes that exist now and did not at the snapshot."""
    return sorted(set(after) - set(before))


def run_teardown(task: GradedTask) -> None:
    """Run the task's teardown, if it has one. A teardown that errors raises `CheckBroken` naming the
    task, because the next session would start on whatever it failed to remove."""
    if not task.teardown:
        return
    try:
        _iad(["exec", task.teardown], task.namespace)
    except CheckBroken as exc:
        raise CheckBroken(f"{task.id} teardown failed: {exc}") from exc


def run_seed(task: GradedTask) -> None:
    """Run the task's seed, if it has one. A seed that errors raises `CheckBroken`: a session on
    data that was not put in place would score the harness, not the agent."""
    if not task.seed:
        return
    try:
        _iad(["exec", task.seed], task.namespace)
    except CheckBroken as exc:
        raise CheckBroken(f"{task.id} seed failed: {exc}") from exc


def validate_live(task: GradedTask) -> None:
    """FR-004 and FR-022 against real IRIS: false before, true after.

    Order matters. The fixture goes on first and the check must fail; then the reference goes over it
    and the same check must pass. The other order would leave the reference in place and prove
    nothing about the precondition.

    The teardown goes first, so state a previous run left behind cannot make the fixture pass.
    """
    run_teardown(task)
    reset_documents([doc.name for doc in task.fixtures], task.namespace)
    apply_documents(task.fixtures, task.namespace)
    run_seed(task)
    if run_check(task):
        raise CorpusInvalid(
            f"{task.id}: FR-004 — the check passes against the untouched fixture, so the task is "
            "already solved before the agent starts and every arm clears it"
        )
    apply_documents(task.solution, task.namespace)
    if not run_check(task):
        raise CorpusInvalid(
            f"{task.id}: FR-022 — the reference solution does not pass this task's own check. Either "
            "the reference is wrong or the check is, and until that is settled a 0.00 from this task "
            "says nothing about the agent"
        )
    failed = [name for name, ok in run_quality(task).items() if ok is not True]
    if failed:
        raise CorpusInvalid(
            f"{task.id}: the reference solution does not pass its own quality check(s) "
            f"{', '.join(failed)}. The reference is the best answer, so a quality check it fails "
            "scores every session wrong"
        )


def _documents(task_id: str, raw, label: str) -> tuple[Document, ...]:
    if not raw:
        return ()
    if not isinstance(raw, list):
        raise CorpusInvalid(f"{task_id}: {label} must be a list of documents")
    documents = []
    for entry in raw:
        if not isinstance(entry, dict) or "name" not in entry or "content" not in entry:
            raise CorpusInvalid(
                f"{task_id}: each {label} entry needs a name and content"
            )
        documents.append(
            Document(name=str(entry["name"]), content=str(entry["content"]))
        )
    return tuple(documents)


def _quality_checks(task_id: str, raw) -> tuple[QualityCheck, ...]:
    if raw is None:
        return ()
    if not isinstance(raw, list):
        raise CorpusInvalid(f"{task_id}: quality_checks must be a list")
    checks = []
    for entry in raw:
        if (
            not isinstance(entry, dict)
            or not str(entry.get("name") or "").strip()
            or not str(entry.get("check") or "").strip()
        ):
            raise CorpusInvalid(
                f"{task_id}: each quality check needs a name and a check"
            )
        checks.append(QualityCheck(name=str(entry["name"]), check=str(entry["check"])))
    return tuple(checks)
