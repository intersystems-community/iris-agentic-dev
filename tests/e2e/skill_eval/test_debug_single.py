"""Debug: print full text passed to judge."""
import os
import pytest

@pytest.mark.skipif(not os.environ.get("OPENAI_API_KEY"), reason="needs key")
def test_debug_ensemble_one_run():
    import sys
    sys.path.insert(0, 'benchmark/021')
    from tests.e2e.skill_eval.evaluator import load_eval_config
    import tests.e2e.skill_eval.lift as lift_mod
    from runner import judge as judge_mod

    orig = judge_mod.score_result
    def patched(task, result):
        t = result.get('transcript', [])
        text = t[0].get('text', '') if t else ''
        print(f"\n{'='*60}")
        print(f"FULL TEXT ({len(text)} chars):")
        print(text)
        print(f"{'='*60}")
        r = orig(task, result)
        print(f"SCORE={r['score']}: {r['reasoning']}")
        return r
    judge_mod.score_result = patched
    lift_mod.score_result = patched

    key = os.environ["OPENAI_API_KEY"]
    model = "openai/gpt-4.1"
    r = lift_mod.run_task_and_score(
        "ENSEMBLE-OBJECTSCRIPT", None, key, model,
        iris_host="localhost", iris_web_port="52780", iris_container="iris-dev-iris",
        no_mcp=True
    )
    print(f"RESULT: {r}")
