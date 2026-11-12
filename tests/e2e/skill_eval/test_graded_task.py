"""Unit tests for the check contract — T027, written before `graded_task.py`.

Three requirements, in test form:

- FR-003, a deterministic check against IRIS state. Validation rejects a task graded by model
  judgement. `benchmark/021`'s tasks carry `expected_behavior` and are scored 0–3 by a judge, and
  those two facts are why nine skills could read 0.00 through a judge that saw 500 characters. A task
  in this corpus states ObjectScript that prints PASS or FAIL, and there is nowhere for an opinion to
  enter.
- FR-004, a precondition that fails before the agent runs. A check that passes against the untouched
  fixture measures nothing — every arm clears it, and the lift is 0.00 whatever the tools do.
- FR-022, a reference solution that passes the task's own check. This is the one thing that separates
  a hard task from a broken check, which is the failure that cost this program four skills.

The live half — actually running the check against the fixture and against the reference — needs
IRIS, because IRIS is the only thing that can answer it. That runs in `test_graded_task_live.py`
against `iris-dev-iris`; it costs no model tokens. What is unit-testable and tested here is the
shape, the PASS/FAIL contract, and the static determinism ban.
"""

import textwrap

import pytest

from tests.e2e.skill_eval.graded_task import (
    BENCHMARK_NAMESPACE,
    CORPUS_DIR,
    PILOT_DIR,
    SKILL_TASK_DIR,
    TASK_DIRS,
    CheckBroken,
    CorpusInvalid,
    all_tasks,
    check_verdict,
    load_dir,
    load_task,
    pilot_tasks,
    skill_corpus,
    tools_corpus,
    validate_shape,
)

TASK = textwrap.dedent(
    """
    id: PILOT-99
    prompt: |
      Fix the loop so it stops on an empty element.
    fixtures:
      - name: Pilot.Sample
        content: |
          Class Pilot.Sample Extends %RegisteredObject
          {
          ClassMethod Count() As %Integer
          {
              Quit 0
          }
          }
    check: |
      write $select(##class(Pilot.Sample).Count()=3:"PASS",1:"FAIL")
    solution:
      - name: Pilot.Sample
        content: |
          Class Pilot.Sample Extends %RegisteredObject
          {
          ClassMethod Count() As %Integer
          {
              Quit 3
          }
          }
    """
).strip()


def write_task(tmp_path, source: str, name: str = "PILOT-99.yaml") -> str:
    path = tmp_path / name
    path.write_text(source + "\n")
    return str(path)


def without(source: str, block: str) -> str:
    """Drop a top-level YAML block by name, keeping the rest — for the missing-field cases."""
    lines = source.splitlines()
    out, skipping = [], False
    for line in lines:
        if line.startswith(f"{block}:"):
            skipping = True
            continue
        if skipping and line and not line[0].isspace():
            skipping = False
        if not skipping:
            out.append(line)
    return "\n".join(out)


# --- the shape a graded task has ----------------------------------------------------------------


def test_a_well_formed_task_loads(tmp_path):
    task = load_task(write_task(tmp_path, TASK))
    assert task.id == "PILOT-99"
    assert "empty element" in task.prompt
    assert task.check.strip().startswith("write")
    assert [f.name for f in task.fixtures] == ["Pilot.Sample"]
    assert [s.name for s in task.solution] == ["Pilot.Sample"]


def test_the_id_must_match_the_filename(tmp_path):
    """A task loaded under one ID and reported under another is a mislabelled row in every table."""
    with pytest.raises(CorpusInvalid) as excinfo:
        load_task(write_task(tmp_path, TASK, name="PILOT-01.yaml"))
    assert "PILOT-99" in str(excinfo.value) and "PILOT-01" in str(excinfo.value)


# --- FR-003: no model judgement --------------------------------------------------------------


def test_a_task_with_no_check_is_rejected(tmp_path):
    with pytest.raises(CorpusInvalid) as excinfo:
        load_task(write_task(tmp_path, without(TASK, "check")))
    assert "check" in str(excinfo.value)


def test_a_task_carrying_expected_behavior_is_rejected(tmp_path):
    """`expected_behavior` is `benchmark/021`'s judge prompt field. Its presence means a human wrote
    the grading criterion for a model to apply, which is the thing FR-003 exists to refuse."""
    with pytest.raises(CorpusInvalid) as excinfo:
        load_task(
            write_task(tmp_path, TASK + '\nexpected_behavior: "the class compiles"\n')
        )
    assert "expected_behavior" in str(excinfo.value)
    assert "FR-003" in str(excinfo.value)


def test_a_task_carrying_a_rubric_is_rejected(tmp_path):
    with pytest.raises(CorpusInvalid):
        load_task(write_task(tmp_path, TASK + "\nrubric:\n  - correctness\n"))


def test_a_task_naming_a_judge_model_is_rejected(tmp_path):
    with pytest.raises(CorpusInvalid):
        load_task(write_task(tmp_path, TASK + "\njudge_model: sonnet\n"))


# --- FR-004: something must be wrong before the agent starts -------------------------------------


def test_a_task_with_no_fixture_is_rejected(tmp_path):
    """No fixture means no untouched state, so there is nothing for the precondition to be false
    against and no way for validation to prove the check is not trivially true."""
    with pytest.raises(CorpusInvalid) as excinfo:
        load_task(write_task(tmp_path, without(TASK, "fixtures")))
    assert "FR-004" in str(excinfo.value)


# --- FR-022: a reference solution, and it must be this task's ------------------------------------


def test_a_task_with_no_reference_solution_is_rejected(tmp_path):
    with pytest.raises(CorpusInvalid) as excinfo:
        load_task(write_task(tmp_path, without(TASK, "solution")))
    assert "FR-022" in str(excinfo.value)


def test_the_reference_solution_must_touch_what_the_fixture_set_up(tmp_path):
    """A solution writing a class the fixture never mentions cannot be the answer to this task; it is
    a second task pasted into the same file."""
    head, _, tail = TASK.partition("solution:")
    wrong = head + "solution:" + tail.replace("Pilot.Sample", "Pilot.Other", 1)
    with pytest.raises(CorpusInvalid) as excinfo:
        load_task(write_task(tmp_path, wrong))
    assert "Pilot.Other" in str(excinfo.value)


# --- determinism ---------------------------------------------------------------------------------


def test_a_check_reading_the_clock_is_rejected(tmp_path):
    """FR-003's word is deterministic. A check that reads `$HOROLOG` gives a different answer
    tomorrow, and a corpus figure that changes with the date cannot be compared to last month's."""
    with pytest.raises(CorpusInvalid) as excinfo:
        load_task(
            write_task(
                tmp_path,
                TASK.replace(
                    'write $select(##class(Pilot.Sample).Count()=3:"PASS",1:"FAIL")',
                    'write $select(##class(Pilot.Sample).Count()=$P($H,",",1):"PASS",1:"FAIL")',
                ),
            )
        )
    assert "$H" in str(excinfo.value)


def test_a_check_rolling_dice_is_rejected(tmp_path):
    with pytest.raises(CorpusInvalid) as excinfo:
        load_task(
            write_task(
                tmp_path,
                TASK.replace(
                    "##class(Pilot.Sample).Count()=3",
                    "##class(Pilot.Sample).Count()=$RANDOM(3)",
                ),
            )
        )
    assert "$RANDOM" in str(excinfo.value)


def test_the_same_output_gives_the_same_verdict_twice():
    output = "PASS"
    assert check_verdict(output) is check_verdict(output) is True


def test_pass_and_fail_are_the_only_two_readings():
    assert check_verdict("PASS") is True
    assert check_verdict("FAIL") is False
    # Real `exec` output has the newline and whatever IRIS echoed around it.
    assert check_verdict("\nPASS\n") is True


def test_output_that_is_neither_is_a_broken_check_not_a_failure():
    """The lesson of the killed-session defect, applied to the checks: a check whose output cannot be
    read has said nothing, and scoring that 0 puts a harness fault in the skill's column."""
    with pytest.raises(CheckBroken) as excinfo:
        check_verdict("<CLASS DOES NOT EXIST> *Pilot.Sample")
    assert "CLASS DOES NOT EXIST" in str(excinfo.value)


def test_a_check_printing_both_words_is_broken():
    with pytest.raises(CheckBroken):
        check_verdict("PASS\nFAIL")


def test_empty_output_is_broken_not_a_failure():
    with pytest.raises(CheckBroken):
        check_verdict("")


# --- the committed pilot corpus ------------------------------------------------------------------


def test_the_pilot_corpus_is_eight_tasks():
    """T026's count, asserted rather than described. Eight is the number the pilot's go/no-go rule in
    T030 is written against — `b >= 5` of eight discordant pairs — so a ninth task or a seventh
    changes what that rule means."""
    tasks = pilot_tasks()
    assert len(tasks) == 8
    assert len({task.id for task in tasks}) == 8


def test_every_committed_pilot_task_passes_shape_validation():
    for task in pilot_tasks():
        validate_shape(task)  # raises CorpusInvalid


def test_every_pilot_check_runs_in_the_benchmark_namespace():
    """One namespace, created and dropped per run. A check reading USER would grade whatever the
    developer left there."""
    for task in pilot_tasks():
        assert task.namespace == BENCHMARK_NAMESPACE


def test_no_pilot_task_names_a_skill():
    """The pilot measures bare against tools. A task written for a skill answers a later question."""
    for task in pilot_tasks():
        assert task.skill is None


# --- the graded corpus: three directories, one task list ------------------------------------------
#
# 121 T031 grows the corpus to 50. The pilot's eight stay where they are, because
# `tests/e2e/results/pilot-121.json` names them and a task that moves directory stops matching its
# own recorded result. So a task lives in one of three places, and which place it is says what
# question it answers:
#
# - `pilot/`   the eight that decided the go/no-go. Historical, and still graded.
# - `corpus/`  the tools ladder: bare against tools against tools+skills.
# - `skills/`  Goal 2 — one named skill each, where the discriminator is a documented convention.
#
# `all_tasks()` is what the split has to cover; `tools_corpus()` and `skill_corpus()` are what the
# two ladders run. The partition is the `skill` field, not the directory, so a skill task filed in
# the wrong place still lands in the right ladder.


def test_the_three_task_directories_are_the_whole_corpus():
    assert TASK_DIRS == (PILOT_DIR, CORPUS_DIR, SKILL_TASK_DIR)


def test_load_dir_reads_every_yaml_file_in_id_order(tmp_path):
    write_task(tmp_path, TASK.replace("PILOT-99", "B-02"), name="B-02.yaml")
    write_task(tmp_path, TASK.replace("PILOT-99", "B-01"), name="B-01.yaml")
    (tmp_path / "notes.md").write_text("not a task\n", encoding="utf-8")
    assert [task.id for task in load_dir(str(tmp_path))] == ["B-01", "B-02"]


def test_load_dir_on_a_missing_directory_is_empty_not_an_error(tmp_path):
    """`corpus/` and `skills/` fill up over two goals. An absent one is a corpus that has not grown
    yet, and raising here would mean the pilot's own tests could not run until both existed."""
    assert load_dir(str(tmp_path / "not-yet")) == ()


def test_a_duplicate_id_across_directories_raises_and_names_it(tmp_path, monkeypatch):
    """The failure this prevents is silent: two tasks under one ID pair against each other across
    arms, and the lift is computed over a row that is two different tasks."""
    one = tmp_path / "one"
    two = tmp_path / "two"
    one.mkdir()
    two.mkdir()
    write_task(one, TASK.replace("PILOT-99", "DUPE-01"), name="DUPE-01.yaml")
    write_task(two, TASK.replace("PILOT-99", "DUPE-01"), name="DUPE-01.yaml")
    monkeypatch.setattr(
        "tests.e2e.skill_eval.graded_task.TASK_DIRS", (str(one), str(two))
    )
    with pytest.raises(CorpusInvalid) as raised:
        all_tasks()
    assert "DUPE-01" in str(raised.value)


def test_all_tasks_is_id_sorted_and_carries_the_pilot():
    tasks = all_tasks()
    assert [task.id for task in tasks] == sorted(task.id for task in tasks)
    assert {task.id for task in pilot_tasks()} <= {task.id for task in tasks}


def test_every_committed_task_passes_shape_validation():
    """The whole corpus, not only the pilot. Shape validation is what stops a judged task or a
    solution-less task reaching a billable session."""
    for task in all_tasks():
        validate_shape(task)  # raises CorpusInvalid


def test_every_committed_check_runs_in_the_benchmark_namespace():
    """Except the AI Hub tasks (132): they run on build 139, where %AI is in USER and there is no
    BENCHMARK namespace to drop. Their teardown does the cleanup the namespace drop does here."""
    for task in all_tasks():
        if task.skill == "iris-ai-hub":
            assert task.namespace == "USER", task.id
            assert task.teardown, task.id
        else:
            assert task.namespace == BENCHMARK_NAMESPACE, task.id


def test_the_two_ladders_partition_the_corpus():
    """A task is in exactly one ladder. Overlap would publish one task's outcome into two figures."""
    tools = {task.id for task in tools_corpus()}
    skills = {task.id for task in skill_corpus()}
    assert not tools & skills
    assert tools | skills == {task.id for task in all_tasks()}


def test_the_tools_ladder_holds_no_task_that_names_a_skill():
    for task in tools_corpus():
        assert task.skill is None


def test_every_skill_ladder_task_names_a_shipped_skill():
    """A task naming a skill that does not ship installs nothing, and the arm it is supposed to
    discriminate is then the tools arm twice."""
    from tests.e2e.skill_eval.pilot import shipped_skills

    shipped = set(shipped_skills())
    assert shipped, (
        "no skills found on disk — the pack moved and this test is now vacuous"
    )
    for task in skill_corpus():
        assert task.skill in shipped, (
            f"{task.id} names {task.skill!r}, which does not ship"
        )


# --- FR-023: a session leaves no class behind ---------------------------------------------------


def test_the_snapshot_covers_each_fixture_top_level_package_once():
    from tests.e2e.skill_eval.graded_task import Document, package_prefixes

    docs = (
        Document(name="Bench.Q2", content=""),
        Document(name="Bench.Sub.Item", content=""),
        Document(name="Other.Thing", content=""),
    )
    assert package_prefixes(docs) == ("Bench", "Other")


def test_only_classes_new_since_the_snapshot_are_created():
    """`Bench.Q2.CountOther` is the 2026-09-27 leftover: new after, absent before. A fixture class the
    session edited was there before and is not the session's to delete."""
    from tests.e2e.skill_eval.graded_task import created_classes

    before = {"Bench.Q2", "Bench.Sub.Item"}
    after = {"Bench.Q2", "Bench.Sub.Item", "Bench.Q2.CountOther", "Bench.Helper"}
    assert created_classes(before, after) == ["Bench.Helper", "Bench.Q2.CountOther"]
    assert created_classes(before, before) == []


def test_a_seed_is_read_and_changes_the_fingerprint_only_when_present(tmp_path):
    """Spec 172 T112: `seed` is ObjectScript that puts the globals a task needs in place after its
    fixtures. A task without one keeps the fingerprint it had before the field existed, so earlier
    runs still pool."""
    from tests.e2e.skill_eval.graded_task import task_fingerprint
    import hashlib
    import json

    plain = load_task(write_task(tmp_path, TASK))
    assert plain.seed is None
    old_body = json.dumps(
        {
            "prompt": plain.prompt,
            "check": plain.check,
            "fixtures": [[d.name, d.content] for d in plain.fixtures],
            "solution": [[d.name, d.content] for d in plain.solution],
            "quality": [[q.name, q.check] for q in plain.quality_checks],
            "namespace": plain.namespace,
            "skill": plain.skill,
            "teardown": plain.teardown,
        },
        sort_keys=True,
    )
    assert task_fingerprint(plain) == hashlib.sha256(old_body.encode("utf-8")).hexdigest()

    seeded = load_task(
        write_task(tmp_path, TASK + '\nseed: |\n  kill ^Pilot99 set ^Pilot99(1)="a^b"\n', name="PILOT-99.yaml")
    )
    assert seeded.seed.strip() == 'kill ^Pilot99 set ^Pilot99(1)="a^b"'
    assert task_fingerprint(seeded) != task_fingerprint(plain)
