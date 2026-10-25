"""Every command the setup skill tells an agent to run has to exist.

A skill is prose until something reads it, and the thing that reads it is a model that will
run what it says without checking. A subcommand that was renamed, a tool that never existed,
an `--args` key the schema rejects: all of that ships silently, and the failure lands on
someone installing for the first time, which is the worst audience to fail in front of.

These checks read the skill the way the CLI would: subcommands against `--help`, tool names
against `tool --list`, and each `--args` payload against that tool's own JSON schema. No IRIS
needed — the schemas come from the binary's router, not from a connection.

The binary comes from `IAD_BINARY`, falling back to `target/debug/iris-agentic-dev`. When
neither exists the tests skip, because a checkout with nothing built cannot answer.
"""

import glob
import json
import os
import re
import subprocess

import pytest

_REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)
#: At `skills/` root, not under `skills/skills/`. The root is what the plugin loader scans
#: implicitly, and a directory under `skills/skills/` would have to be added to `bundled.rs`
#: to keep `embedded_catalog_matches_the_skills_directory_on_disk` green. A setup skill also
#: has no business being in the binary's catalog: it exists to be read before the binary is
#: installed, so `skill install` could never be how someone gets it.
_SKILL = os.path.join(_REPO_ROOT, "skills", "iris-agentic-dev-setup", "SKILL.md")

#: `iris-agentic-dev <word>` in a command someone types. The word after the binary name is a
#: subcommand unless the binary is being passed as an argument to something else. Spaces and
#: tabs only, never a newline: `\s+` reaches across a line break and reads the first word of
#: the next command as a subcommand of the previous one.
_SUBCOMMAND_RE = re.compile(r"\biris-agentic-dev[ \t]+([a-z][a-z-]*)")

#: `tool <name> --args '<json>'`, the one shape that carries a payload.
_TOOL_CALL_RE = re.compile(r"\btool\s+([a-z_][a-z0-9_]*)\s+--args\s+'(.*?)'", re.S)

#: Words that follow the binary name without being subcommands: flags take values, and
#: `claude mcp add iris-agentic-dev ...` passes the name as a server label.
_NOT_SUBCOMMANDS = frozenset({"mcp"})

#: Lines where the binary name is an argument rather than the program being run.
_NAME_AS_ARGUMENT = ("claude mcp",)

#: The one tap that exists. Homebrew expands it to `intersystems-community/homebrew-tap`.
_HOMEBREW_TAP = "intersystems-community/tap"

#: `brew tap <name>` as a command. The leading guard keeps the prose word "Homebrew tap" out,
#: which otherwise reads as a tap named by whatever word follows it.
_BREW_TAP_RE = re.compile(r"(?<![\w-])brew tap\s+(\S+)")

#: Fence languages holding commands a reader types. `text` and `json` fences are output.
_COMMAND_FENCES = frozenset({"bash", "sh", "shell", "console"})

#: The release workflow's upload list is the only place that knows what a tag publishes, so it
#: is what the docs have to agree with. Reading it keeps the check offline: asking GitHub would
#: pass on a stale release and fail on a runner with no network.
_RELEASE_WORKFLOW = os.path.join(_REPO_ROOT, ".github", "workflows", "release.yml")

#: `iris-agentic-dev-<platform>` as it appears in an asset name or a download URL.
_ASSET_RE = re.compile(r"iris-agentic-dev-(?:linux|macos|windows)-[a-z0-9_]+(?:\.exe)?")


def _released_assets() -> set[str]:
    with open(_RELEASE_WORKFLOW, encoding="utf-8") as handle:
        assets = set(_ASSET_RE.findall(handle.read()))
    assert assets, f"{_RELEASE_WORKFLOW} names no release assets; the regex has rotted"
    return assets


def _binary() -> str:
    candidate = os.environ.get("IAD_BINARY") or os.path.join(
        _REPO_ROOT, "target", "debug", "iris-agentic-dev"
    )
    if not os.path.isfile(candidate):
        pytest.skip(
            f"{candidate} is not built; set IAD_BINARY or run `cargo build` to check the "
            "skill's commands against the real CLI surface"
        )
    return candidate


def _skill_text() -> str:
    # An assertion rather than a skip. The skill is a file in this repo, so "it is not here"
    # is a failure, not an unavailable prerequisite — a skip would let the whole feature
    # vanish while CI stayed green.
    assert os.path.exists(_SKILL), (
        f"{os.path.relpath(_SKILL, _REPO_ROOT)} does not exist. FR-005 requires it, and the "
        "plugin loader scans `skills/` root, so that is where it belongs."
    )
    with open(_SKILL, encoding="utf-8") as handle:
        return handle.read()


def _typed_commands(skill: str) -> str:
    """Prose and command fences only, with output fences dropped.

    An expected-output line like `Added stdio MCP server iris-agentic-dev with command:`
    parses to a regex as the subcommand `with`. Output is not something a reader types.
    """
    kept: list[str] = []
    fence: str | None = None
    # Frontmatter is metadata, and its description is a sentence. "Install the
    # iris-agentic-dev binary" reads to the regex as the subcommand `binary`.
    body = skill.split("---", 2)[2] if skill.startswith("---\n") else skill
    for line in body.splitlines():
        if line.startswith("```"):
            fence = None if fence is not None else (line[3:].strip().lower() or "none")
            continue
        if fence is not None and fence not in _COMMAND_FENCES:
            continue
        if any(marker in line for marker in _NAME_AS_ARGUMENT):
            continue
        kept.append(line)
    return "\n".join(kept)


def _cli_subcommands(binary: str) -> set[str]:
    result = subprocess.run(
        [binary, "--help"], capture_output=True, text=True, timeout=120
    )
    section = result.stdout.split("Commands:", 1)
    assert len(section) == 2, f"`{binary} --help` printed no Commands section"
    names = set()
    for line in section[1].splitlines():
        if not line.startswith("  ") or not line.strip():
            if names:
                break
            continue
        names.add(line.split()[0])
    return names


def _cli_tools(binary: str) -> set[str]:
    result = subprocess.run(
        [binary, "tool", "--list"], capture_output=True, text=True, timeout=120
    )
    assert result.returncode == 0, f"`{binary} tool --list` failed: {result.stderr}"
    return {line.split()[0] for line in result.stdout.splitlines() if line.strip()}


def _tool_schema(binary: str, tool: str) -> dict:
    result = subprocess.run(
        [binary, "tool", tool, "--schema", "--json"],
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert result.returncode == 0, (
        f"`tool {tool} --schema --json` failed: {result.stderr}"
    )
    payload = json.loads(result.stdout)
    return payload.get("inputSchema") or payload.get("input_schema") or payload


@pytest.mark.requires_binary
def test_every_subcommand_the_skill_names_exists():
    # The skill is read before the binary is looked for, so a missing skill fails rather
    # than skipping on a checkout with nothing built.
    named = (
        set(_SUBCOMMAND_RE.findall(_typed_commands(_skill_text()))) - _NOT_SUBCOMMANDS
    )
    binary = _binary()
    unknown = sorted(named - _cli_subcommands(binary))
    assert not unknown, (
        f"the setup skill tells an agent to run subcommands the CLI does not have: {unknown}"
    )


@pytest.mark.requires_binary
def test_every_tool_the_skill_names_exists():
    named = {match[0] for match in _TOOL_CALL_RE.findall(_skill_text())}
    binary = _binary()
    unknown = sorted(named - _cli_tools(binary))
    assert not unknown, (
        f"the setup skill calls tools that are not in the catalogue: {unknown}"
    )


@pytest.mark.requires_binary
def test_every_args_payload_parses_and_uses_declared_fields():
    calls = _TOOL_CALL_RE.findall(_skill_text())
    binary = _binary()
    for tool, raw in calls:
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as exc:
            pytest.fail(f"--args for {tool} is not valid JSON: {exc}\n{raw}")
        schema = _tool_schema(binary, tool)
        declared = set(schema.get("properties", {}))
        undeclared = sorted(set(payload) - declared)
        assert not undeclared, (
            f"--args for {tool} passes {undeclared}, which the tool does not declare. Every "
            "tool sets additionalProperties: false, so the call is rejected with "
            f"UNKNOWN_PARAMETER. Declared: {sorted(declared)}."
        )
        missing = sorted(set(schema.get("required", [])) - set(payload))
        assert not missing, f"--args for {tool} omits required fields {missing}"


def test_every_document_names_the_homebrew_tap_that_exists():
    """FR-006. One tap, and `brew tap` fails loudly on the other.

    `intersystems-community/tap` resolves to the `homebrew-tap` repository.
    `intersystems-community/iris-agentic-dev` resolves to `homebrew-iris-agentic-dev`, which
    does not exist, so anyone following that line gets a 404 on their first command. The tap
    name is the same in every document, so a repo-wide scan is the right shape of check.
    """
    wrong: list[str] = []
    # Tracked plus untracked-but-not-ignored: git-ignored local scratch (.iad-local/) never ships.
    listed = subprocess.run(
        [
            "git",
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            "*.md",
        ],
        cwd=_REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    for relpath in filter(None, listed.split("\0")):
        path = os.path.join(_REPO_ROOT, relpath)
        if not os.path.isfile(path):
            continue
        with open(path, encoding="utf-8", errors="replace") as handle:
            for number, line in enumerate(handle, start=1):
                for tap in re.findall(_BREW_TAP_RE, line):
                    if tap != _HOMEBREW_TAP:
                        wrong.append(f"{relpath}:{number}: {tap}")
    assert not wrong, (
        f"these lines name a Homebrew tap that is not {_HOMEBREW_TAP}, so `brew tap` fails "
        f"for anyone who follows them: {wrong}"
    )


def test_the_skill_downloads_assets_a_release_actually_publishes():
    """A wrong asset name is a 404 on the reader's first command.

    `releases/latest/download/<name>` does not fail usefully: GitHub answers 404 and `curl -f`
    leaves an empty file behind, so the next line reports a permission or exec error instead of
    a missing download.
    """
    named = set(_ASSET_RE.findall(_skill_text()))
    assert named, (
        "the skill names no download asset, so step 2 cannot work without Homebrew"
    )
    unknown = sorted(named - _released_assets())
    assert not unknown, (
        f"the skill downloads assets no release publishes: {unknown}. A tag builds only what "
        f"{os.path.relpath(_RELEASE_WORKFLOW, _REPO_ROOT)} uploads."
    )


def test_the_skill_covers_every_platform_a_release_builds():
    """Measured in a container with no Homebrew: the binaries are per-architecture.

    The `linux-aarch64` asset runs on an ARM Linux host and `linux-x86_64` does not, so a skill
    that names only the x86_64 one sends every ARM Linux reader to `cannot execute binary
    file`. Requiring full coverage also means adding a platform to the release cannot ship
    without the install instructions following it.
    """
    missing = sorted(_released_assets() - set(_ASSET_RE.findall(_skill_text())))
    assert not missing, (
        f"a release publishes these and the setup skill never mentions them: {missing}. "
        "Anyone on that platform has no install command."
    )


def test_the_skill_has_frontmatter_a_loader_can_read():
    skill = _skill_text()
    assert skill.startswith("---\n"), "a SKILL.md has to open with YAML frontmatter"
    frontmatter = skill.split("---", 2)[1]
    assert re.search(r"^name:\s*\S", frontmatter, re.M), "frontmatter needs a name"
    description = re.search(r"^description:\s*(.+)$", frontmatter, re.M)
    assert description, "frontmatter needs a description"
    assert "use when" in description.group(1).lower(), (
        "a skill description has to say when to use it, or the model reads the description "
        "and skips the body — see skills/BENCHMARKING.md"
    )


# --------------------------------------------------------------------------------------------
# Spec 141 US5 (FR-029): what the shipped setup material leaves behind on a container.
#
# The checks below read every file iad ships that starts or configures a container. They need no
# binary and no Docker, so they cannot skip: a skill that publishes a port on every interface or
# sets the published default password fails here, not on a stranger's laptop. The live proof that
# the corrected steps work is in `test_setup_skill_live.py`.
# --------------------------------------------------------------------------------------------

#: Output fences hold what a command printed, not something a reader runs.
_OUTPUT_FENCES = frozenset({"text", "output", "txt"})

_SCANNED_RS = (
    os.path.join("crates", "iris-agentic-dev-bin", "src", "cmd", "mcp.rs"),
    os.path.join("crates", "iris-agentic-dev-bin", "src", "cmd", "init.rs"),
)

#: Files whose whole text (prose included) must not tell a reader to set the default password or
#: put one in a workspace file. These are the files a reader follows to get a first connection.
_SETUP_PATHS = (
    os.path.join("skills", "iris-agentic-dev-setup", "SKILL.md"),
    os.path.join("skills", "skills", "iris-agentic-dev", "nopws-setup", "SKILL.md"),
    os.path.join("docs", "getting-started.md"),
    os.path.join("docs", "connecting.md"),
    "README.md",
    *_SCANNED_RS,
)

#: The `_SYSTEM` hash CPFPreset ships. It is the hash of the published default password, so a
#: skill that shows it teaches the reader to set `SYS`.
_DEFAULT_PASSWORD_HASH = "FBFE8593AEFA510C27FD184738D6E865A441DE98"

_LOOPBACK_PREFIXES = ("127.0.0.1:", "[::1]:")


def _shipped_files() -> list[str]:
    """Repo-relative paths of every shipped file that may start or configure a container."""
    found: list[str] = []
    for pattern in (
        ("skills", "**", "SKILL.md"),
        ("skills", "*.md"),
        ("docs", "*.md"),
        ("README.md",),
    ):
        matches = glob.glob(os.path.join(_REPO_ROOT, *pattern), recursive=True)
        assert matches, (
            f"the scan pattern {'/'.join(pattern)} matches nothing; it has rotted"
        )
        found.extend(os.path.relpath(path, _REPO_ROOT) for path in matches)
    for relpath in _SCANNED_RS:
        assert os.path.isfile(os.path.join(_REPO_ROOT, relpath)), f"{relpath} is gone"
        found.append(relpath)
    return sorted(set(found))


def _read(relpath: str) -> str:
    with open(os.path.join(_REPO_ROOT, relpath), encoding="utf-8") as handle:
        return handle.read()


def _fenced_blocks(relpath: str) -> list[tuple[int, str]]:
    """(first line number, text) of each fenced block in a Markdown file, output fences dropped.

    A `.rs` file has no fences; its whole text is one block, because its help strings and
    doc comments are what a reader sees.
    """
    text = _read(relpath)
    if relpath.endswith(".rs"):
        return [(1, text)]
    blocks: list[tuple[int, str]] = []
    fence: str | None = None
    start = 0
    body: list[str] = []
    for number, line in enumerate(text.splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("```"):
            if fence is None:
                fence = (
                    stripped[3:].strip().lower().split()[0]
                    if stripped[3:].strip()
                    else "none"
                )
                start = number + 1
                body = []
            else:
                if fence not in _OUTPUT_FENCES:
                    blocks.append((start, "\n".join(body)))
                fence = None
            continue
        if fence is not None:
            body.append(line)
    return blocks


def _logical_lines(first_line: int, block: str) -> list[tuple[int, str]]:
    """Lines of a block with backslash continuations joined, each with its first line number."""
    joined: list[tuple[int, str]] = []
    pending: list[str] = []
    pending_start = first_line
    for offset, line in enumerate(block.splitlines()):
        if not pending:
            pending_start = first_line + offset
        if line.rstrip().endswith("\\"):
            pending.append(line.rstrip()[:-1])
            continue
        pending.append(line)
        joined.append((pending_start, " ".join(part.strip() for part in pending)))
        pending = []
    if pending:
        joined.append((pending_start, " ".join(part.strip() for part in pending)))
    return joined


_PUBLISH_RE = re.compile(r"(?:^|\s)(?:-p|--publish)(?:=|\s+)(\S+)")
_PUBLISH_ALL_RE = re.compile(r"(?:^|\s)(?:-P|--publish-all)(?:\s|$)")
_COMPOSE_PORT_ITEM_RE = re.compile(r"^\s*-\s*[\"']?([^\"'\s#]+)")
_COMPOSE_PORTS_KEY_RE = re.compile(r"^(\s*)ports:\s*(\[.*\])?\s*(#.*)?$")


def _compose_port_entries(first_line: int, block: str) -> list[tuple[int, str]]:
    entries: list[tuple[int, str]] = []
    lines = block.splitlines()
    index = 0
    while index < len(lines):
        match = _COMPOSE_PORTS_KEY_RE.match(lines[index])
        if match is None:
            index += 1
            continue
        indent, inline = len(match.group(1)), match.group(2)
        if inline:
            for item in inline.strip("[]").split(","):
                item = item.strip().strip("\"'")
                if item:
                    entries.append((first_line + index, item))
            index += 1
            continue
        index += 1
        while index < len(lines):
            line = lines[index]
            item = _COMPOSE_PORT_ITEM_RE.match(line)
            if not line.strip():
                index += 1
                continue
            if item is None or len(line) - len(line.lstrip()) <= indent:
                break
            entries.append((first_line + index, item.group(1)))
            index += 1
    return entries


def test_shipped_files_publish_on_loopback_only():
    """FR-029: every published port in a shipped file binds 127.0.0.1.

    `-p 1972:1972` publishes on every host interface, and on Linux Docker's own iptables rules
    sit ahead of ufw and firewalld, so "the machine has a firewall" protects nobody. A container
    a skill starts is for one developer on one machine, so the bind address says so.
    """
    open_binds: list[str] = []
    for relpath in _shipped_files():
        for first_line, block in _fenced_blocks(relpath):
            for number, line in _logical_lines(first_line, block):
                if re.search(r"\bdocker\s+(?:container\s+)?(?:run|create)\b", line):
                    for value in _PUBLISH_RE.findall(line):
                        if not value.startswith(_LOOPBACK_PREFIXES):
                            open_binds.append(f"{relpath}:{number}: -p {value}")
                    if _PUBLISH_ALL_RE.search(line):
                        open_binds.append(
                            f"{relpath}:{number}: -P publishes every port"
                        )
            for number, entry in _compose_port_entries(first_line, block):
                if not entry.startswith(_LOOPBACK_PREFIXES):
                    open_binds.append(f"{relpath}:{number}: ports: {entry}")
    assert not open_binds, (
        "these shipped lines publish a container port on every host interface; prefix each "
        f"with 127.0.0.1: {open_binds}"
    )


_UNEXPIRE_ALL_RE = re.compile(r"UnExpireUserPasswords\(\s*\\?\"\*\\?\"\s*[,)]")


def test_no_shipped_file_unexpires_every_account():
    """FR-029: no shipped command clears the expiry on every predefined account.

    `UnExpireUserPasswords("*")` leaves `_SYSTEM`, `SuperUser`, `Admin` and `CSPSystem` with
    their published passwords and no expiry. The expiry on those accounts is the only thing
    that stops `SYS` working on a container someone else can reach.
    """
    found: list[str] = []
    for relpath in _shipped_files():
        for first_line, block in _fenced_blocks(relpath):
            for offset, line in enumerate(block.splitlines()):
                if _UNEXPIRE_ALL_RE.search(line):
                    found.append(f"{relpath}:{first_line + offset}")
    assert not found, (
        f"these lines un-expire every account, so every predefined user keeps `SYS`: {found}"
    )


_SYS_CHANGE_RE = re.compile(
    r"ChangePassword\(\s*\\?\"[^\"]*\\?\"\s*,\s*\\?\"SYS\\?\"\s*,\s*\\?\"SYS\\?\""
)
_CONNECTION_PASSWORD_RE = re.compile(
    r"""(?:^|[\s"'])(?:IRIS_PASSWORD(?:=|"?\s*:\s*"?)|password\s*=\s*")SYS\b"""
)


def test_no_shipped_file_sets_the_default_password():
    """FR-029: no shipped step sets, resets or keeps the published default password.

    Covers each form the earlier skills used: a CSP.ini line, a reset to `SYS`, the hash of
    `SYS`, an expiry switched off with no new hash beside it, a docker run with
    `IRIS_PASSWORD=SYS`, and `SYS` written into a connection example a reader copies.
    """
    found: list[str] = []
    for relpath in _shipped_files():
        for first_line, block in _fenced_blocks(relpath):
            for number, line in _logical_lines(first_line, block):
                if re.search(r"(?<![A-Za-z])Password=SYS\b", line):
                    found.append(f"{relpath}:{number}: Password=SYS")
                if _SYS_CHANGE_RE.search(line):
                    found.append(f"{relpath}:{number}: ChangePassword(...,SYS,SYS)")
                if "PasswordNeverExpires=1" in line and "PasswordHash=" not in line:
                    found.append(
                        f"{relpath}:{number}: PasswordNeverExpires=1 with no PasswordHash"
                    )
                if re.search(r"new_password\s*=\s*[\"']SYS[\"']", line):
                    found.append(
                        f"{relpath}:{number}: reset_password(new_password=SYS)"
                    )
                if (
                    re.search(r"\bdocker\s+run\b", line)
                    and "IRIS_PASSWORD=SYS" in line
                    and "iris-agentic-dev" not in line
                ):
                    found.append(f"{relpath}:{number}: docker run -e IRIS_PASSWORD=SYS")
        # The hash of `SYS` is a recipe in prose too; anything that prints it is a copy source.
        if _DEFAULT_PASSWORD_HASH in _read(relpath).upper():
            found.append(f"{relpath}: prints the hash of the default password")
    for relpath in _SETUP_PATHS:
        for number, line in enumerate(_read(relpath).splitlines(), start=1):
            if _CONNECTION_PASSWORD_RE.search(line):
                found.append(f"{relpath}:{number}: {line.strip()[:80]}")
    assert not found, (
        f"these shipped lines set or teach the default password: {sorted(found)}"
    )


def _section(text: str, heading_start: str) -> str:
    """The text from a `## <heading_start>...` heading up to the next `## ` heading."""
    match = re.search(
        rf"^## {re.escape(heading_start)}.*?$(.*?)(?=^## |\Z)", text, re.M | re.S
    )
    assert match, f"the setup skill has no `## {heading_start}` section"
    return match.group(1)


def test_setup_skill_asks_before_reusing_a_container():
    """FR-029: an existing container is the user's, so the skill asks and changes nothing.

    The old step 3 said "If there is exactly one, use it and go to step 4", and step 4 changed
    the accounts of whatever step 3 found. That is a password change on a container the skill
    did not start, on the word of a `docker ps` filter.
    """
    skill = _skill_text()
    find = _section(skill, "3.")
    assert "go to step 4" not in find, (
        "step 3 still sends the reader to step 4 with a container the skill did not start"
    )
    assert "ask" in find.lower(), (
        "step 3 never says to ask the user about an existing container"
    )
    assert "never" in find.lower(), (
        "step 3 has to say what it never does to an existing container (change its accounts, "
        "stop it, recreate it)"
    )
    password_step = _section(skill, "4.")
    assert "iris-quickstart" in password_step, (
        "step 4 has to name the container the skill started, not 'the container from step 3'"
    )
    assert "docker exec" in password_step and "iris-quickstart" in password_step
