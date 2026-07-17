"""Probe: does Sonnet use TO_VECTOR(?,DOUBLE,384) or TO_VECTOR(?) for Python code?"""
import os, shutil, tempfile
import pytest

@pytest.mark.skipif(not os.environ.get("OPENAI_API_KEY"), reason="needs key")
def test_tovector_arg_count():
    from tests.e2e.isolated_env import IsolatedEnv
    from tests.e2e.opencode_runner import collect_events

    key = os.environ["OPENAI_API_KEY"]
    model = "amazon-bedrock/us.anthropic.claude-sonnet-4-5-20250929-v1:0"
    prompt = (
        "Write Python code using iris.dbapi to search a RAG.Documents table "
        "for the 5 nearest neighbors to a 384-dim query vector. "
        "The embedding column is VECTOR(DOUBLE, 384). "
        "Pass the query as a comma-separated string."
    )

    for label, install_skill in [("BASELINE", False), ("WITH_SKILL", True)]:
        workdir = tempfile.mkdtemp(prefix=f"probe-{label}-")
        try:
            with IsolatedEnv(openai_api_key=key) as env:
                if install_skill:
                    dest = os.path.join(env.skills_dir, "iris-vector-ai")
                    os.makedirs(dest, exist_ok=True)
                    shutil.copy2(
                        "skills/skills/iris-vector-ai/SKILL.md",
                        os.path.join(dest, "SKILL.md"),
                    )
                events = collect_events(prompt, env.env_vars(), model=model, working_dir=workdir)

            texts = [
                e["part"]["text"]
                for e in events
                if e.get("type") == "text" and e["part"].get("time", {}).get("end")
            ]
            full = "\n".join(texts)
            print(f"\n=== {label} ===")
            found = {}
            for kw in [
                "TO_VECTOR(?,DOUBLE", "TO_VECTOR(?, DOUBLE",   # 3-arg correct
                "TO_VECTOR(?)",                                  # 1-arg wrong
                "::vector", "<=>",                              # pgvector wrong
                "LIMIT", "TOP ",                                # LIMIT=pgvector, TOP=IRIS
                "VECTOR_COSINE",                                # correct
            ]:
                found[kw] = kw.lower() in full.lower()
                if found[kw]:
                    print(f"  FOUND: {repr(kw)}")
            # Print SQL lines
            for line in full.splitlines():
                if any(x in line for x in ["TO_VECTOR", "<=>", "VECTOR_COSINE", "LIMIT", "TOP "]):
                    print(f"  > {line.strip()[:120]}")
        finally:
            shutil.rmtree(workdir, ignore_errors=True)
