"""Load and validate benchmark task YAML files."""
import os
import re
import yaml

REQUIRED_FIELDS = ("id", "category", "description", "expected_behavior", "path")
VALID_CATEGORIES = {"GEN", "MOD", "DBG", "SCM", "LEG", "PYPR", "COV"}
VALID_PATHS = {"A", "B", "both"}
ID_PATTERN = re.compile(r"^[A-Z]+-[0-9]+$")

_DEFAULT_TASKS_DIR = os.path.join(os.path.dirname(__file__), "..", "tasks")


def load_tasks(
    tasks_dir: str = None,
    path_filter: str = "both",
    category_filter: list = None,
    task_id: str = None,
) -> list:
    tasks_dir = tasks_dir or _DEFAULT_TASKS_DIR
    tasks = []

    for fname in sorted(os.listdir(tasks_dir)):
        if not fname.endswith(".yaml"):
            continue
        with open(os.path.join(tasks_dir, fname)) as f:
            task = yaml.safe_load(f)

        _validate(task, fname)

        # single task filter
        if task_id and task["id"] != task_id:
            continue

        # path filter
        task_path = task.get("path", "both")
        if path_filter != "both" and task_path not in ("both", path_filter):
            continue

        # category filter
        if category_filter and task["category"] not in category_filter:
            continue

        tasks.append(task)

    return tasks


def _validate(task: dict, fname: str):
    for field in REQUIRED_FIELDS:
        if field not in task or not task[field]:
            raise ValueError(f"{fname}: missing required field '{field}'")

    if not ID_PATTERN.match(task["id"]):
        raise ValueError(f"{fname}: id '{task['id']}' must match [A-Z]+-[0-9]+")

    if task["category"] not in VALID_CATEGORIES:
        raise ValueError(f"{fname}: unknown category '{task['category']}'")

    if task.get("path", "both") not in VALID_PATHS:
        raise ValueError(f"{fname}: path must be A, B, or both")
