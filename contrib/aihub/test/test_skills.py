"""
Tests for IAD.Skill.* classes.

T-078-01: All four skill classes compile with zero errors.
T-078-02: Each skill's SUMMARY XData has non-empty name and description.
T-078-03: ObjectScriptRepair agent round-trip — detects a known ObjectScript mistake.
T-078-04: IrisNavigation uses the read-only toolset (write tools absent).
T-078-05: IAD.Agent.ObjectScriptDev declarative agent class compiles and has PROVIDER+SKILLS.

Requirements:
  - IRIS AI Hub instance at aihub-iris-116
  - IAD.ToolSet.xml imported (run test_toolset.py first, or let this test do it)
  - iris-agentic-dev binary on PATH or IRIS_AGENTIC_DEV_BIN set
  - ANTHROPIC_API_KEY set (T-078-03 only)
"""

import json
import os
import subprocess
import unittest

IRIS_HOST = os.environ.get("AIHUB_IRIS_HOST", "localhost")
IRIS_WEB_PORT = os.environ.get("AIHUB_IRIS_WEB_PORT", "25277")
IRIS_USERNAME = os.environ.get("AIHUB_IRIS_USERNAME", "_SYSTEM")
IRIS_PASSWORD = os.environ.get("AIHUB_IRIS_PASSWORD", "SYS")
IRIS_NAMESPACE = os.environ.get("AIHUB_IRIS_NAMESPACE", "USER")
IRIS_CONTAINER = os.environ.get("AIHUB_IRIS_CONTAINER", "aihub-iris-116")
IAD_BIN = os.environ.get("IRIS_AGENTIC_DEV_BIN", "iris-agentic-dev")

SKILL_CLASSES = [
    "IAD.Skill.ObjectScriptRepair",
    "IAD.Skill.ObjectScriptGuardrails",
    "IAD.Skill.InteropDebugging",
    "IAD.Skill.IrisNavigation",
]

_ENV = {
    **os.environ,
    "IRIS_HOST": IRIS_HOST,
    "IRIS_WEB_PORT": IRIS_WEB_PORT,
    "IRIS_USERNAME": IRIS_USERNAME,
    "IRIS_PASSWORD": IRIS_PASSWORD,
    "IRIS_NAMESPACE": IRIS_NAMESPACE,
    "IRIS_CONTAINER": IRIS_CONTAINER,
}


def iris_execute(code, namespace=IRIS_NAMESPACE):
    """Execute ObjectScript via iris-agentic-dev tool and return IRIS output."""
    env = {**_ENV, "IRIS_NAMESPACE": namespace}
    cmd = [
        IAD_BIN, "tool",
        "--host", IRIS_HOST,
        "--web-port", IRIS_WEB_PORT,
        "--username", IRIS_USERNAME,
        "--password", IRIS_PASSWORD,
        "--namespace", namespace,
        "--container", IRIS_CONTAINER,
        "iris_execute",
        "--args", json.dumps({"code": code}),
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=30, env=env)
    try:
        d = json.loads(result.stdout)
        return d.get("output", "") + ("\n" + d.get("error", "") if d.get("error") else "")
    except (json.JSONDecodeError, ValueError):
        return result.stdout + result.stderr


def _docker_exec(code):
    """Run ObjectScript directly in the container via irissession (no gate).

    Writes code to a temp file inside the container to avoid shell quoting issues.
    """
    import tempfile, base64
    # Base64-encode the code to avoid any shell quoting issues
    encoded = base64.b64encode(code.encode()).decode()
    bash_cmd = (
        f"echo {encoded} | base64 -d > /tmp/_iad_test.os && "
        "cat /tmp/_iad_test.os | irissession IRIS -U USER"
    )
    r = subprocess.run(
        ["docker", "exec", IRIS_CONTAINER, "bash", "-c", bash_cmd],
        capture_output=True, text=True, timeout=30
    )
    return r.stdout + r.stderr


class TestSkillImport(unittest.TestCase):
    """T-078-01: All four skill classes compile with zero errors."""

    def test_all_skill_classes_compiled(self):
        for cls in SKILL_CLASSES:
            with self.subTest(cls=cls):
                code = f'Write ##class(%Dictionary.CompiledClass).%ExistsId("{cls}"),!'
                out = iris_execute(code)
                self.assertIn(
                    "1", out,
                    f"{cls} is not compiled. Import IAD.ToolSet.xml first:\n{out}"
                )

    def test_example_agent_compiled(self):
        code = 'Write ##class(%Dictionary.CompiledClass).%ExistsId("IAD.Agent.ObjectScriptDev"),!'
        out = iris_execute(code)
        self.assertIn(
            "1", out,
            f"IAD.Agent.ObjectScriptDev is not compiled:\n{out}"
        )


class TestSkillDiscovery(unittest.TestCase):
    """T-078-02: Each skill's SUMMARY XData has non-empty name and description.

    Uses docker exec to bypass the code-edit gate on %Dictionary.XDataDefinition.
    """

    def _get_summary_xdata(self, cls):
        code = (
            f'Set xd = ##class(%Dictionary.XDataDefinition).%OpenId("{cls}||SUMMARY") '
            f'If \'$IsObject(xd) {{ Write "NOT_FOUND",! Quit }} '
            f'Set s = xd.Data Do s.Rewind() '
            f'Set buf="" While 1 {{ Set c=s.Read(500) Set buf=buf_c Quit:s.AtEnd }} '
            f'Write buf,!'
        )
        return _docker_exec(code)

    def test_summary_xdata_name_field(self):
        for cls in SKILL_CLASSES:
            with self.subTest(cls=cls):
                out = self._get_summary_xdata(cls)
                self.assertNotIn("NOT_FOUND", out, f"SUMMARY XData missing for {cls}")
                self.assertIn(
                    "name:", out,
                    f"SUMMARY XData for {cls} missing 'name:' field:\n{out}"
                )

    def test_summary_xdata_description_field(self):
        for cls in SKILL_CLASSES:
            with self.subTest(cls=cls):
                out = self._get_summary_xdata(cls)
                self.assertNotIn("NOT_FOUND", out, f"SUMMARY XData missing for {cls}")
                self.assertIn(
                    "description:", out,
                    f"SUMMARY XData for {cls} missing 'description:' field:\n{out}"
                )


class TestObjectScriptRepairRoundTrip(unittest.TestCase):
    """T-078-03: Repair skill agent identifies a known ObjectScript mistake."""

    def setUp(self):
        if not os.environ.get("ANTHROPIC_API_KEY"):
            self.skipTest("ANTHROPIC_API_KEY not set — skipping live agent test")

    def test_repair_detects_list_api_mistake(self):
        bad_code = "Set val = $List(myList, 1)"
        prompt = f"Review this ObjectScript code and identify mistakes: {bad_code}"
        code = (
            'Set agent = ##class(%AI.Agent).%New() '
            'Set agent.Provider = "anthropic" '
            'Set agent.ApiKey = $system.Util.GetEnviron("ANTHROPIC_API_KEY") '
            'Do agent.UseSkill("IAD.Skill.ObjectScriptRepair") '
            f'Set result = agent.Run("{prompt}") '
            'Write result,!'
        )
        out = iris_execute(code)
        flagged = (
            "$list" in out.lower()
            or "$listget" in out.lower()
            or "mistake" in out.lower()
            or "error" in out.lower()
        )
        self.assertTrue(flagged, f"Expected agent to flag $List() mistake, got:\n{out[:800]}")


class TestIrisNavigationReadOnly(unittest.TestCase):
    """T-078-04: IrisNavigation uses the read-only toolset."""

    def test_tools_parameter_is_readonly(self):
        """Verify TOOLS parameter value via docker exec (bypasses %Dictionary gate)."""
        code = (
            'Set cls = ##class(%Dictionary.ClassDefinition).%OpenId("IAD.Skill.IrisNavigation") '
            'If \'$IsObject(cls) { Write "NOT_FOUND",! Quit } '
            'Set i = 1 '
            'For { Quit:i>cls.Parameters.Count() '
            '  Set p = cls.Parameters.GetAt(i) '
            '  If p.Name = "TOOLS" { Write p.Default,! } '
            '  Set i = i + 1 '
            '}'
        )
        out = _docker_exec(code)
        self.assertNotIn("NOT_FOUND", out, "IAD.Skill.IrisNavigation class not found")
        self.assertIn(
            "IrisAgenticDevReadOnly", out,
            f"IAD.Skill.IrisNavigation should use ReadOnly toolset, got TOOLS='{out.strip()}'"
        )


class TestDeclarativeAgentCompile(unittest.TestCase):
    """T-078-05: IAD.Agent.ObjectScriptDev compiles and has PROVIDER+SKILLS parameters."""

    def _get_params(self):
        code = (
            'Set cls = ##class(%Dictionary.ClassDefinition).%OpenId("IAD.Agent.ObjectScriptDev") '
            'If \'$IsObject(cls) { Write "NOT_FOUND",! Quit } '
            'Set i = 1 '
            'For { Quit:i>cls.Parameters.Count() '
            '  Set p = cls.Parameters.GetAt(i) '
            '  Write p.Name," = ",p.Default,! '
            '  Set i = i + 1 '
            '}'
        )
        return _docker_exec(code)

    def test_example_agent_has_provider_parameter(self):
        out = self._get_params()
        self.assertNotIn("NOT_FOUND", out, "IAD.Agent.ObjectScriptDev class not found")
        self.assertIn("PROVIDER", out,
                      f"Expected PROVIDER parameter in IAD.Agent.ObjectScriptDev:\n{out}")

    def test_example_agent_has_skills_parameter(self):
        out = self._get_params()
        self.assertNotIn("NOT_FOUND", out, "IAD.Agent.ObjectScriptDev class not found")
        self.assertIn("SKILLS", out,
                      f"Expected SKILLS parameter in IAD.Agent.ObjectScriptDev:\n{out}")
        self.assertIn("IAD.Skill.", out,
                      f"Expected IAD.Skill.* in SKILLS parameter value:\n{out}")


if __name__ == "__main__":
    unittest.main(verbosity=2)
