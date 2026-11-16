"""ROADMAP.md stays current: it is reviewed at every release, and nothing it links to is gone.

The roadmap names the version it was last reviewed for. That line has to equal the workspace
version in Cargo.toml, so the release that bumps the version cannot pass CI without someone
reading the roadmap again (release checklist, section 3). The link checks catch the other way a
public roadmap goes stale: an item pointing at a spec or document that was renamed or removed.
"""

import datetime
import os
import re

_REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)
_ROADMAP = os.path.join(_REPO_ROOT, "ROADMAP.md")
_MONTHS = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
]
_BLOCK = re.compile(r"^## (" + "|".join(_MONTHS) + r") (\d{4}): (\S.*)$", re.M)


def _read(*parts):
    with open(os.path.join(_REPO_ROOT, *parts), encoding="utf-8") as f:
        return f.read()


def _workspace_version():
    m = re.search(r'^version\s*=\s*"([^"]+)"', _read("Cargo.toml"), re.M)
    assert m, "no version in Cargo.toml"
    return m.group(1)


def _roadmap():
    assert os.path.isfile(_ROADMAP), "ROADMAP.md is missing from the repository root"
    return _read("ROADMAP.md")


def test_roadmap_was_reviewed_for_the_current_version():
    m = re.search(
        r"^\*\*Reviewed for:\*\* v(\d+\.\d+\.\d+) on (\d{4}-\d{2}-\d{2})",
        _roadmap(),
        re.M,
    )
    assert m, "ROADMAP.md needs a line like '**Reviewed for:** v1.7.1 on 2026-11-03'"
    version, day = m.groups()
    assert version == _workspace_version(), (
        f"ROADMAP.md was last reviewed for v{version}, the workspace is "
        f"v{_workspace_version()}: review it and update the line (release checklist, section 3)"
    )
    datetime.date.fromisoformat(day)


def _reviewed_on():
    m = re.search(
        r"^\*\*Reviewed for:\*\* v\S+ on (\d{4}-\d{2}-\d{2})", _roadmap(), re.M
    )
    assert m, "ROADMAP.md has no '**Reviewed for:**' line"
    return datetime.date.fromisoformat(m.group(1))


def test_roadmap_plans_in_consecutive_monthly_blocks():
    """One themed block per month (`## November 2026: security`), at least three, back to back."""
    blocks = [
        (int(y), _MONTHS.index(mo) + 1, theme)
        for mo, y, theme in _BLOCK.findall(_roadmap())
    ]
    assert len(blocks) >= 3, f"expected at least three monthly blocks, found {blocks}"
    for (y1, m1, _), (y2, m2, _) in zip(blocks, blocks[1:]):
        assert (y2, m2) == (
            (y1, m1 + 1) if m1 < 12 else (y1 + 1, 1)
        ), f"monthly blocks are not consecutive: {y1}-{m1:02d} then {y2}-{m2:02d}"


def test_first_block_is_the_month_of_the_review():
    """A block for a month already over means the roadmap was not moved forward."""
    first = _BLOCK.search(_roadmap())
    assert first, "ROADMAP.md has no monthly block"
    day = _reviewed_on()
    month = (int(first.group(2)), _MONTHS.index(first.group(1)) + 1)
    assert month in [
        (day.year, day.month),
        (day.year + (day.month == 12), day.month % 12 + 1),
    ], (
        f"the first block is {first.group(1)} {first.group(2)}, the roadmap was reviewed on {day}: "
        "drop past months and start at the current one"
    )


def test_backlog_and_shipped_follow_the_monthly_blocks():
    text = _roadmap()
    headings = re.findall(r"^## (.+)$", text, re.M)
    tail = [h for h in headings if not _BLOCK.match("## " + h)]
    assert tail == [
        "Backlog",
        "Shipped",
    ], f"after the monthly blocks expected Backlog, Shipped; found {tail}"
    last_block = list(_BLOCK.finditer(text))[-1].start()
    assert text.index("## Backlog") > last_block


def test_every_relative_link_in_the_roadmap_exists():
    missing = []
    for target in re.findall(r"\]\(([^)\s]+)\)", _roadmap()):
        if re.match(r"[a-z]+:", target) or target.startswith("#"):
            continue
        path = target.split("#", 1)[0]
        if not os.path.exists(os.path.join(_REPO_ROOT, path)):
            missing.append(target)
    assert not missing, f"ROADMAP.md links to paths that do not exist: {missing}"


def test_every_spec_the_roadmap_names_exists():
    missing = [
        s
        for s in set(re.findall(r"specs/\d{3}-[a-z0-9-]+", _roadmap()))
        if not os.path.isdir(os.path.join(_REPO_ROOT, s))
    ]
    assert (
        not missing
    ), f"ROADMAP.md names specs that are not in the repository: {missing}"


def test_shipped_links_the_current_release_notes():
    shipped = _roadmap().split("## Shipped", 1)[1]
    notes = f"docs/release-notes/v{_workspace_version()}.md"
    assert notes in shipped, f"the Shipped section should link {notes}"


def test_readme_and_release_checklist_point_at_the_roadmap():
    assert "ROADMAP.md" in _read("README.md"), "README.md does not link ROADMAP.md"
    assert "ROADMAP.md" in _read(
        "docs", "release-checklist.md"
    ), "docs/release-checklist.md has no step to review ROADMAP.md"
