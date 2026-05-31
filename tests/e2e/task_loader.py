"""Load HarnessTask YAML fixtures."""
import os
import yaml
from dataclasses import dataclass, field


@dataclass
class HarnessAssertion:
    type: str
    pattern: str
    description: str
    required: bool = True


@dataclass
class HarnessFixture:
    type: str
    name: str
    content: str


@dataclass
class HarnessTask:
    id: str
    category: str
    scenario: str
    description: str
    prompt: str
    fixtures: list[HarnessFixture] = field(default_factory=list)
    skills_to_install: list[str] = field(default_factory=list)
    assertions: list[HarnessAssertion] = field(default_factory=list)
    expected_tool_calls: list[str] = field(default_factory=list)
    model: str | None = None  # per-task model override


def load_task(path: str) -> HarnessTask:
    with open(path) as f:
        data = yaml.safe_load(f)
    return HarnessTask(
        id=data["id"],
        category=data["category"],
        scenario=data["scenario"],
        description=data["description"],
        prompt=data["prompt"].strip(),
        fixtures=[HarnessFixture(**fx) for fx in data.get("fixtures", [])],
        skills_to_install=data.get("skills_to_install", []),
        assertions=[HarnessAssertion(**a) for a in data.get("assertions", [])],
        expected_tool_calls=data.get("expected_tool_calls", []),
        model=data.get("model"),
    )


TASKS_DIR = os.path.join(os.path.dirname(__file__), "tasks")
