"""Spec 141 US5 (FR-029): follow the shipped setup steps on a container that is really new.

`iris-dev-iris` has been configured for years and cannot show a first boot, so these tests start
a throwaway container, run the commands the skills print, and check what the container looks like
afterwards: which interfaces it listens on, and which accounts still log in with `SYS`.

The commands come out of the skill files, not out of this file. A test that carried its own copy
would stay green while the skill rotted.

Selected with `-m live`. They need Docker, and a missing Docker fails them: they exist to show
the steps work, and a skip shows nothing. `IAD_ALLOW_SKIP=1` opts into a skip on a host that has
no Docker. CI excludes them through `requires_iris`.

The container is `iad-throwaway-141`, published on 127.0.0.1:39780 (superserver) and
127.0.0.1:39781 (web), the pair registered for this spec. The skill's own `docker run` publishes
1972 and 52773 on the host, which another project on the machine may hold; `_rebind` swaps in the
test ports and keeps the bind address, which is the part under test. `IAD_SETUP141_IMAGE`
replaces the image the skill names, for a host whose cached `latest` is stale.

Run under the shared lock:
    lockf -k /tmp/iad-141-iris.lock python3 -m pytest tests/e2e/test_setup_skill_live.py -m live
"""

from __future__ import annotations

import os
import re
import secrets
import shutil
import socket
import subprocess
import time
import urllib.error
import urllib.request

import pytest

pytestmark = [pytest.mark.live, pytest.mark.requires_iris]

_REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)
_SETUP_SKILL = os.path.join(_REPO_ROOT, "skills", "iris-agentic-dev-setup", "SKILL.md")
_CPF_SKILL = os.path.join(
    _REPO_ROOT, "skills", "skills", "iris-cpf-merge", "SKILL.md"
)

CONTAINER = "iad-throwaway-141"
SUPERSERVER_PORT = 39780
WEB_PORT = 39781
_HOST_PORT = {"1972": SUPERSERVER_PORT, "52773": WEB_PORT}
#: The accounts that ship with the published password. After the setup steps only the one the
#: user chose may log in with it.
_OTHER_ACCOUNTS = ("SuperUser", "Admin", "CSPSystem")


def _require_docker() -> None:
    if shutil.which("docker") is not None:
        probe = subprocess.run(
            ["docker", "info"], capture_output=True, text=True, timeout=60
        )
        if probe.returncode == 0:
            return
    if os.environ.get("IAD_ALLOW_SKIP") == "1":
        pytest.skip("Docker is not available (IAD_ALLOW_SKIP=1)")
    pytest.fail(
        "Docker is not available. These tests start a throwaway IRIS to prove the setup steps "
        "work; without Docker they prove nothing, so they fail. Set IAD_ALLOW_SKIP=1 to skip."
    )


def _docker(*args: str, stdin: str | None = None, timeout: int = 120):
    return subprocess.run(
        ["docker", *args], input=stdin, capture_output=True, text=True, timeout=timeout
    )


def _remove() -> None:
    subprocess.run(
        ["docker", "rm", "-f", CONTAINER], capture_output=True, text=True, timeout=120
    )


@pytest.fixture
def throwaway():
    """No container before, none after, whether the test passed or failed."""
    _require_docker()
    _remove()
    try:
        yield CONTAINER
    finally:
        _remove()


def _fenced_blocks(path: str) -> list[str]:
    blocks: list[str] = []
    body: list[str] | None = None
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            if line.strip().startswith("```"):
                if body is None:
                    body = []
                else:
                    blocks.append("".join(body))
                    body = None
            elif body is not None:
                body.append(line)
    return blocks


def _the_block(path: str, *needles: str) -> str:
    matching = [b for b in _fenced_blocks(path) if all(n in b for n in needles)]
    assert len(matching) == 1, (
        f"{os.path.relpath(path, _REPO_ROOT)} must have exactly one fenced block containing "
        f"{needles}; it has {len(matching)}"
    )
    return matching[0]


_PUBLISH_RE = re.compile(
    r"(?P<flag>-p|--publish)(?P<sep>[ =])(?P<ip>[\d.]+:)?(?:\d*:)?(?P<port>1972|52773)\b"
)


def _rebind(command: str) -> str:
    """Swap the host ports and the container name for this test's; keep the bind address."""

    def swap(match: re.Match) -> str:
        return (
            f"{match['flag']}{match['sep']}{match['ip'] or ''}"
            f"{_HOST_PORT[match['port']]}:{match['port']}"
        )

    command = _PUBLISH_RE.sub(swap, command)
    command = command.replace("iris-quickstart", CONTAINER).replace("my-iris", CONTAINER)
    image = os.environ.get("IAD_SETUP141_IMAGE")
    if image:
        command = re.sub(r"intersystemsdc/iris-community:\S+", image, command)
    return command


def _status(user: str, password: str, port: int = WEB_PORT) -> int:
    """HTTP status of `GET /api/atelier/` with these credentials; 0 when nothing answers."""
    request = urllib.request.Request(f"http://127.0.0.1:{port}/api/atelier/")
    token = f"{user}:{password}".encode()
    import base64

    request.add_header("Authorization", "Basic " + base64.b64encode(token).decode())
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status
    except urllib.error.HTTPError as err:
        return err.code
    except (urllib.error.URLError, OSError):
        return 0


def _wait_ready(timeout: int = 300) -> None:
    """IRIS answers HTTP 401 well before `iris session` works, and a bad image can shut itself
    down after that. Ready is: web answers, a session runs code, and both hold for three probes
    spread over fifteen seconds."""
    deadline = time.time() + timeout
    good = 0
    while time.time() < deadline:
        running = _docker("inspect", "-f", "{{.State.Running}}", CONTAINER)
        if running.stdout.strip() != "true":
            logs = _docker("logs", "--tail", "15", CONTAINER)
            pytest.fail(
                f"{CONTAINER} is not running (the image stopped itself during start):\n"
                f"{logs.stdout}{logs.stderr}"
            )
        ready = _status("_SYSTEM", "SYS") in (200, 401) and "READY" in _docker(
            "exec", "-i", CONTAINER, "iris", "session", "IRIS", "-U", "%SYS",
            stdin='Write "READY",!\nHalt\n',
        ).stdout
        good = good + 1 if ready else 0
        if good >= 3:
            return
        time.sleep(5)
    logs = _docker("logs", "--tail", "15", CONTAINER)
    pytest.fail(f"{CONTAINER} was not ready in {timeout}s:\n{logs.stdout}{logs.stderr}")


def _lan_address() -> str | None:
    """The address this host uses to reach a network, or None on a host with only loopback."""
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        probe.connect(("10.255.255.255", 1))
        address = probe.getsockname()[0]
    except OSError:
        return None
    finally:
        probe.close()
    return None if address.startswith("127.") else address


def _reachable(address: str, port: int) -> bool:
    try:
        socket.create_connection((address, port), timeout=3).close()
        return True
    except OSError:
        return False


def _run_bash(script: str, timeout: int = 300) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["bash", "-c", script], capture_output=True, text=True, timeout=timeout
    )


def test_following_the_setup_skill_leaves_one_new_password(throwaway):
    """L-A1, L-A3 and L-A6: the skill's own commands, run on a fresh container.

    Afterwards the container listens on loopback only, `_SYSTEM` logs in with the password the
    skill generated, and `_SYSTEM`, `SuperUser`, `Admin` and `CSPSystem` all refuse `SYS`.
    """
    start = _rebind(_the_block(_SETUP_SKILL, "docker run"))
    started = _run_bash(start)
    assert started.returncode == 0, f"the skill's docker run failed:\n{started.stderr}"
    _wait_ready()

    assert _status("_SYSTEM", "SYS") == 401, (
        "a fresh container must refuse the expired default password, or the password step has "
        "nothing to change"
    )

    step = _rebind(_the_block(_SETUP_SKILL, "openssl rand", "iris session IRIS -U"))
    changed = _run_bash(f"{step}\nprintf 'IAD_PW=%s\\n' \"$PW\"\n")
    password = next(
        (l[len("IAD_PW="):] for l in changed.stdout.splitlines() if l.startswith("IAD_PW=")),
        None,
    )
    assert password, f"the skill's password block left no $PW:\n{changed.stdout}{changed.stderr}"
    assert password != "SYS" and len(password) >= 20

    # L-A1 and L-A3: the new password works while the account started expired, and nothing
    # logs in with SYS any more.
    _assert_only_this_password_works(password)

    # L-A6: the ports are on loopback, and the LAN address is refused.
    ports = _docker("port", CONTAINER).stdout.splitlines()
    assert ports, "docker port printed nothing"
    for line in ports:
        assert "-> 127.0.0.1:" in line, f"published beyond loopback: {line}"
    lan = _lan_address()
    if lan is None:
        if os.environ.get("IAD_ALLOW_SKIP") != "1":
            pytest.fail("this host has no non-loopback address, so L-A6 cannot be checked")
    else:
        assert _reachable("127.0.0.1", WEB_PORT), "loopback must still answer"
        assert not _reachable(lan, WEB_PORT), (
            f"{lan}:{WEB_PORT} accepted a connection; the web port is on every interface"
        )
        assert not _reachable(lan, SUPERSERVER_PORT), (
            f"{lan}:{SUPERSERVER_PORT} accepted a connection; the superserver is on every "
            "interface"
        )


def _skill_image() -> str:
    """The image the setup skill starts, unless IAD_SETUP141_IMAGE replaces it."""
    override = os.environ.get("IAD_SETUP141_IMAGE")
    if override:
        return override
    match = re.search(
        r"intersystemsdc/iris-community:\S+", _the_block(_SETUP_SKILL, "docker run")
    )
    assert match, "the setup skill's docker run names no intersystemsdc/iris-community image"
    return match.group(0)


def _assert_only_this_password_works(password: str) -> None:
    assert _status("_SYSTEM", password) == 200, "the new password does not log in"
    assert _status("_SYSTEM", "SYS") == 401, "SYS still logs in as _SYSTEM"
    for account in _OTHER_ACCOUNTS:
        assert _status(account, "SYS") == 401, f"{account}:SYS still logs in"


def test_iris_password_env_sets_the_password(throwaway):
    """L-A2: `-e IRIS_PASSWORD=<pw>` sets `_SYSTEM`'s password and clears its expiry.

    The setup skill says this works and why it prefers the stdin step anyway: the value lands in
    `docker inspect` and in the `docker run` command line.
    """
    password = "La2-" + secrets.token_urlsafe(14)
    started = _docker(
        "run", "-d", "--name", CONTAINER,
        "-p", f"127.0.0.1:{SUPERSERVER_PORT}:1972", "-p", f"127.0.0.1:{WEB_PORT}:52773",
        "-e", f"IRIS_PASSWORD={password}", _skill_image(),
    )  # fmt: skip
    assert started.returncode == 0, started.stderr
    _wait_ready()
    _assert_only_this_password_works(password)
    inspected = _docker("inspect", "-f", "{{.Config.Env}}", CONTAINER).stdout
    assert password in inspected, (
        "docker inspect no longer shows IRIS_PASSWORD, so the skill's warning about it is stale"
    )


def test_cpf_merge_skill_sets_the_password_without_a_hash_line(tmp_path, throwaway):
    """L-A4, as it turned out: a merge line cannot set a generated password, `IRIS_PASSWORD` can.

    The research note expected a `PasswordHash=` line built from a generated password to take
    effect at first boot. On `intersystemsdc/iris-community:2026.2` it did not: hex and base64
    PBKDF2 hashes, with and without the algorithm and work-factor fields, were ignored or left
    `_SYSTEM` with a password nobody knew. `Security.Users` has no `GeneratePasswordHash`.
    So `iris-cpf-merge` says to set the password with `IRIS_PASSWORD` (or the stdin step)
    and keep the merge file to services and namespaces. This test runs the skill's own command
    with a merge file mounted, and checks both halves: the merge ran, and only the new
    password works.
    """
    merge_dir = tmp_path / "merge"
    merge_dir.mkdir(mode=0o755)
    merge_file = merge_dir / "merge.cpf"
    merge_file.write_text(
        "[Actions]\nModifyService:Name=%Service_CallIn,Enabled=1,AutheEnabled=48\n"
    )
    merge_file.chmod(0o644)

    password = "La4-" + secrets.token_urlsafe(14)
    command = _rebind(_the_block(_CPF_SKILL, "IRIS_PASSWORD", "docker run"))
    command = command.replace(
        "docker run -d",
        f"docker run -d -v {merge_dir}:/merge:ro -e ISC_CPF_MERGE_FILE=/merge/merge.cpf",
    )
    started = subprocess.run(
        ["bash", "-c", command],
        capture_output=True,
        text=True,
        timeout=120,
        env={**os.environ, "IRIS_PASSWORD": password},
    )
    assert started.returncode == 0, f"the skill's docker run failed:\n{started.stderr}"
    _wait_ready()

    _assert_only_this_password_works(password)
    logs = _docker("logs", CONTAINER)
    assert "Merging /merge/merge.cpf" in logs.stdout + logs.stderr, (
        "the merge file was mounted but IRIS never merged it"
    )
