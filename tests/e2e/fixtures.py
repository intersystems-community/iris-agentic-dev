"""Load HarnessTask fixtures into IRIS via Atelier REST (no LLM)."""
import requests
from tests.e2e.task_loader import HarnessFixture


def load_fixture(
    fixture: HarnessFixture,
    iris_host: str,
    iris_web_port: str,
    iris_namespace: str = "USER",
    iris_username: str = "_SYSTEM",
    iris_password: str = "SYS",
) -> None:
    """Write a fixture into IRIS via Atelier REST PUT + compile."""
    auth = (iris_username, iris_password)
    base = f"http://{iris_host}:{iris_web_port}/api/atelier/v1/{iris_namespace}"

    if fixture.type == "cls":
        doc_name = fixture.name + ".cls"
        url = f"{base}/doc/{doc_name}"
        payload = {
            "enc": False,
            "content": fixture.content.splitlines(),
        }
        r = requests.put(url, json=payload, auth=auth, timeout=30)
        r.raise_for_status()

        compile_url = f"{base}/action/compile"
        r2 = requests.post(compile_url, json=[doc_name], auth=auth, timeout=30)
        r2.raise_for_status()
    else:
        raise NotImplementedError(f"Fixture type '{fixture.type}' not yet supported")


def load_all_fixtures(
    fixtures: list[HarnessFixture],
    iris_host: str,
    iris_web_port: str,
    **kwargs,
) -> None:
    for fixture in fixtures:
        load_fixture(fixture, iris_host=iris_host, iris_web_port=iris_web_port, **kwargs)
