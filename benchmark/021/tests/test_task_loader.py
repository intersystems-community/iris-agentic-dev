"""T008 — Unit tests for task_loader.py."""
import os
import pytest
import tempfile
import yaml


VALID_TASK = {
    "id": "GEN-01",
    "category": "GEN",
    "path": "both",
    "description": "Write a simple class",
    "expected_behavior": "Class compiles",
}


def _write_tasks(tmp_dir, tasks):
    for t in tasks:
        with open(os.path.join(tmp_dir, f"{t['id']}.yaml"), "w") as f:
            yaml.dump(t, f)


def test_loads_valid_task():
    with tempfile.TemporaryDirectory() as d:
        _write_tasks(d, [VALID_TASK])
        from task_loader import load_tasks
        tasks = load_tasks(tasks_dir=d)
    assert len(tasks) == 1
    assert tasks[0]["id"] == "GEN-01"


def test_filters_by_path_a():
    tasks = [
        {**VALID_TASK, "id": "GEN-01", "path": "A"},
        {**VALID_TASK, "id": "GEN-02", "path": "B"},
        {**VALID_TASK, "id": "GEN-03", "path": "both"},
    ]
    with tempfile.TemporaryDirectory() as d:
        _write_tasks(d, tasks)
        from task_loader import load_tasks
        result = load_tasks(tasks_dir=d, path_filter="A")
    ids = {t["id"] for t in result}
    assert "GEN-01" in ids
    assert "GEN-03" in ids
    assert "GEN-02" not in ids


def test_filters_by_category():
    tasks = [
        {**VALID_TASK, "id": "GEN-01", "category": "GEN"},
        {**VALID_TASK, "id": "MOD-01", "category": "MOD"},
    ]
    with tempfile.TemporaryDirectory() as d:
        _write_tasks(d, tasks)
        from task_loader import load_tasks
        result = load_tasks(tasks_dir=d, category_filter=["MOD"])
    assert all(t["category"] == "MOD" for t in result)


def test_raises_on_missing_required_field():
    bad_task = {"id": "BAD-01", "category": "GEN"}  # missing description
    with tempfile.TemporaryDirectory() as d:
        _write_tasks(d, [bad_task])
        from task_loader import load_tasks
        with pytest.raises(ValueError, match="description"):
            load_tasks(tasks_dir=d)


def test_filter_by_task_id():
    tasks = [
        {**VALID_TASK, "id": "GEN-01"},
        {**VALID_TASK, "id": "GEN-02"},
    ]
    with tempfile.TemporaryDirectory() as d:
        _write_tasks(d, tasks)
        from task_loader import load_tasks
        result = load_tasks(tasks_dir=d, task_id="GEN-01")
    assert len(result) == 1
    assert result[0]["id"] == "GEN-01"


def test_doc_category_is_valid():
    task = {**VALID_TASK, "id": "DOC-01", "category": "DOC"}
    with tempfile.TemporaryDirectory() as d:
        _write_tasks(d, [task])
        from task_loader import load_tasks
        result = load_tasks(tasks_dir=d)
    assert len(result) == 1
    assert result[0]["category"] == "DOC"


def test_doc_category_filter_works():
    tasks = [
        {**VALID_TASK, "id": "DOC-01", "category": "DOC"},
        {**VALID_TASK, "id": "GEN-01", "category": "GEN"},
    ]
    with tempfile.TemporaryDirectory() as d:
        _write_tasks(d, tasks)
        from task_loader import load_tasks
        result = load_tasks(tasks_dir=d, category_filter=["DOC"])
    assert len(result) == 1
    assert result[0]["id"] == "DOC-01"
