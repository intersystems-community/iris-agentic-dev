"""SKILL-74, the lift task for spec 173's `propose` index linking (T064).

The task puts a master global and an index global on a server with a class that maps only the master,
and asks for lookups by name to use the index. Nothing here needs IRIS: the shape, the split and the
answers the check is written against. Whether the check is false on the stub and true on the reference is
`graded_task.validate_live`, which costs no model tokens and runs before any session.

The known answers come from the spec 173 fixture seed (`F02.json`), not from anything the task prints.
"""

from __future__ import annotations

import json
import os

import pytest

from tests.e2e.skill_eval import graded_task
from tests.e2e.skill_eval.ladder import skill_ladder_tasks
from tests.e2e.skill_eval.split import default_split

TASK_ID = "SKILL-74"
SKILL = "iris-storage-mapping"

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
TASK_PATH = os.path.join(graded_task.AUTHORED_DIR, SKILL, f"{TASK_ID}.yaml")
F02_SEED = os.path.join(
    REPO,
    "crates",
    "iris-agentic-dev-core",
    "tests",
    "fixtures",
    "storage173",
    "seed",
    "F02.json",
)


@pytest.fixture(scope="module")
def task():
    return graded_task.load_task(TASK_PATH)


@pytest.fixture(scope="module")
def f02():
    with open(F02_SEED, encoding="utf-8") as handle:
        return json.load(handle)


def test_the_task_file_exists_under_its_skill_directory():
    assert os.path.isfile(TASK_PATH), TASK_PATH


def test_the_task_is_in_the_authored_corpus_under_its_own_skill():
    tasks = {t.id: t for t in graded_task.load_authored()}
    assert tasks[TASK_ID].skill == SKILL


def test_the_id_is_not_declared_twice_across_the_whole_corpus():
    # all_tasks raises on a duplicate id; SKILL-30 is taken by objectscript-sql-patterns.
    ids = [t.id for t in graded_task.all_tasks()]
    assert ids.count(TASK_ID) == 1


def test_the_task_is_a_holdout_task_in_the_storage_mapping_ladder():
    assert default_split().side_of(TASK_ID) == "holdout"
    assert TASK_ID in [t.id for t in skill_ladder_tasks(SKILL)]


def test_the_task_runs_in_benchmark_with_a_seed_and_a_teardown(task):
    assert task.namespace == "BENCHMARK"
    assert task.seed and task.teardown
    assert "iris-dev-iris" in task.prompt and "52780" in task.prompt


def test_the_prompt_says_what_the_developer_has_and_wants_and_leaks_nothing(task):
    prompt = task.prompt.lower()
    assert "^iadsm173person" in prompt and "^iadsm173personx" in prompt
    for leak in (
        "sqlupper",
        "upper",
        "collat",
        "iris_storage_map",
        "propose",
        "sqlmap",
        "<subscript",
        "expression",
        "transform",
    ):
        assert leak not in prompt, f"the prompt names {leak!r}"


def test_the_fixture_maps_only_the_master_and_the_reference_adds_the_index_map(
    task, f02
):
    fixture = task.fixtures[0].content
    assert "^IadSm173Person" in fixture and "^IadSm173PersonX" not in fixture
    assert fixture.count("<SQLMap ") == 1 and "<Type>data</Type>" in fixture
    solution = task.solution[0].content
    assert solution.count("<SQLMap ") == 2
    assert "<Global>^IadSm173PersonX</Global>" in solution
    assert "<Expression>$$SQLUPPER({Name})</Expression>" in solution
    assert task.solution[0].name == task.fixtures[0].name
    assert f02["answer"]["transform"] == "sqlupper" and f02["answer"]["field"] == "Name"


def test_the_seed_holds_the_fixture_rows_and_an_index_that_needs_the_transform(
    task, f02
):
    # 40 rows: the linker's 20-node floor is met with room.
    assert len(f02["rows"]) == 40
    for row in f02["rows"][:20]:
        if row[0].isascii():
            assert row[0] in task.seed, row[0]
    assert "kill ^IadSm173Person" in task.seed
    assert "SQLUPPER" in task.seed
    assert "^IadSm173PersonX(" in task.seed


def test_the_check_names_the_known_rows_from_the_fixture_seed(task, f02):
    # The expected strings are rebuilt from the fixture seed's rows (ID is the row's 1-based position,
    # City is column 1), so a check edited to agree with whatever IRIS returns still has to match.
    def expected(name):
        return ",".join(
            f"{i + 1}:{row[1]}"
            for i, row in enumerate(f02["rows"])
            if row[0].upper() == name
        )

    brown = expected("BROWN, CY")
    assert brown == "8:Paris,9:Kyiv"
    assert expected("SMITH, ANNA") == "1:Lyon"
    check = task.check
    assert "Name = 'brown, cy'" in check and f'got="{brown}"' in check
    assert "Name = 'smith, anna'" in check and 'got2="1:Lyon"' in check
    # the index global still holds every row, and the master's row 8 is untouched
    assert "n=40" in check and 'Brown, Cy' in check


def test_the_check_reads_the_planner_and_the_stored_index_map_and_calls_no_iad_tool(
    task,
):
    check = task.check
    assert '"EXPLAIN "_q' in check
    assert "Read index map" in check
    assert "^IadSm173PersonX" in check
    assert "SQLMaps" in check and "%Dictionary.CompiledClass" in check
    lowered = check.lower()
    for tool in (
        "iris_storage_map",
        "iris_doc",
        "iris_execute",
        "iris-agentic-dev",
        "iad ",
    ):
        assert tool not in lowered, tool
    assert check.rstrip().endswith('write $select(ok:"PASS",1:"FAIL")')


def test_the_teardown_kills_every_global_the_task_or_a_wrong_answer_can_leave(task):
    for name in ("^IadSm173Person", "^IadSm173PersonX"):
        assert name in task.teardown
    assert "kill " in task.teardown and "teardown done" in task.teardown
