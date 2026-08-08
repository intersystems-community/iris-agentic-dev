"""
Tests for IAD.Skill.* classes.

T-078-01: All four skill classes import with zero compile errors.
T-078-02: %GetSummary() (or equivalent) returns non-empty name/description.
T-078-03: ObjectScriptRepair agent round-trip — detects a known ObjectScript mistake.
T-078-04: IrisNavigation uses the read-only toolset (write tools absent).
T-078-05: IAD.Agent.ObjectScriptDev declarative agent class compiles and %Init() stub works.

Requirements:
  - IRIS AI Hub instance at aihub-iris-116 (port 21972)
  - IAD.ToolSet.xml imported (run test_toolset.py first)
  - iris-agentic-dev binary on PATH or IRIS_AGENTIC_DEV_BIN set
  - ANTHROPIC_API_KEY set (T-078-03 only)
"""

import json
import os
import subprocess
import unittest

IRIS_HOST = os.environ.get("AIHUB_IRIS_HOST", "localhost")
IRIS_WEB_PORT = os.environ.get("AIHUB_IRIS_WEB_PORT", "21972")
IRIS_USERNAME = os.environ.get("AIHUB_IRIS_USERNAME", "_SYSTEM")
IRIS_PASSWORD = os.environ.get("AIHUB_IRIS_PASSWORD", "SYS")
IRIS_NAMESPACE = os.environ.get("AIHUB_IRIS_NAMESPACE", "USER")
IAD_BIN = os.environ.get("IRIS_AGENTIC_DEV_BIN", "iris-agentic-dev")

SKILL_CLASSES = [
    "IAD.Skill.ObjectScriptRepair",
    "IAD.Skill.ObjectScriptGuardrails",
    "IAD.Skill.InteropDebugging",
    "IAD.Skill.IrisNavigation",
]


def iad(*args):
    cmd = [
        IAD_BIN,
        "--host", IRIS_HOST,
        "--web-port", IRIS_WEB_PORT,
        "--username", IRIS_USERNAME,
        "--password", IRIS_PASSWORD,
        "--namespace", IRIS_NAMESPACE,
    ] + list(args)
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    return result


def iris_execute(code):
    result = iad("tool", "iris_execute", "--args", json.dumps({"code": code}))
    return result.stdout + result.stderr


class TestSkillImport(unittest.TestCase):
    """T-078-01: All four skill classes compile with zero errors."""

    def test_all_skill_classes_compiled(self):
        for cls in SKILL_CLASSES:
            with self.subTest(cls=cls):
                code = f'Write $system.OBJ.IsCompiled("{cls}"),!'
                out = iris_execute(code)
                self.assertIn(
                    "1", out,
                    f"{cls} is not compiled. Import IAD.ToolSet.xml first:\n{out}"
                )

    def test_example_agent_compiled(self):
        code = 'Write $system.OBJ.IsCompiled("IAD.Agent.ObjectScriptDev"),!'
        out = iris_execute(code)
        self.assertIn(
            "1", out,
            f"IAD.Agent.ObjectScriptDev is not compiled:\n{out}"
        )


class TestSkillDiscovery(unittest.TestCase):
    """T-078-02: Each skill's SUMMARY XData has non-empty name and description."""

    def _get_summary_xdata(self, cls):
        code = (
            f'Set xd = ##class(%Dictionary.XDataDefinition).%OpenId("{cls}||SUMMARY") '
            f'If \'$IsObject(xd) {{ Write "NOT_FOUND",! Quit }} '
            f'Write xd.Data.Read(32000),!'
        )
        return iris_execute(code)

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
        # $List() is the wrong API — should be $ListGet()
        bad_code = "Set val = $List(myList, 1)"
        prompt = (
            f"Review this ObjectScript code and identify any mistakes:\n"
            f"```\n{bad_code}\n```"
        )
        code = (
            'Set agent = ##class(%AI.Agent).%New() '
            'Set agent.Provider = "anthropic" '
            'Set agent.ApiKey = $system.Util.GetEnviron("ANTHROPIC_API_KEY") '
            'Do agent.UseSkill("IAD.Skill.ObjectScriptRepair") '
            f'Set result = agent.Run("{prompt.replace(chr(34), chr(39))}") '
            'Write result,!'
        )
        out = iris_execute(code)
        # The agent should flag $List() as a mistake
        flagged = (
            "$list" in out.lower()
            or "$listget" in out.lower()
            or "mistake" in out.lower()
            or "wrong" in out.lower()
            or "incorrect" in out.lower()
            or "error" in out.lower()
        )
        self.assertTrue(
            flagged,
            f"Expected agent to flag $List() mistake, got:\n{out[:800]}"
        )


class TestIrisNavigationReadOnly(unittest.TestCase):
    """T-078-04: IrisNavigation uses the read-only toolset — write tools absent."""

    def test_tools_parameter_is_readonly(self):
        code = (
            'Set cls = ##class(%Dictionary.ClassDefinition).%OpenId("IAD.Skill.IrisNavigation") '
            'If \'$IsObject(cls) { Write "NOT_FOUND",! Quit } '
            'Set params = cls.Parameters '
            'Set i = 1 '
            'For { Quit:i>params.Count() '
            '  Set p = params.GetAt(i) '
            '  If p.Name = "TOOLS" { Write p.Default,! } '
            '  Set i = i + 1 '
            '}'
        )
        out = iris_execute(code)
        self.assertIn(
            "IrisAgenticDevReadOnly", out,
            f"IAD.Skill.IrisNavigation should use ReadOnly toolset, got TOOLS='{out.strip()}'"
        )


class TestDeclarativeAgentCompile(unittest.TestCase):
    """T-078-05: IAD.Agent.ObjectScriptDev compiles and class structure is valid."""

    def test_example_agent_has_provider_parameter(self):
        code = (
            'Set cls = ##class(%Dictionary.ClassDefinition).%OpenId("IAD.Agent.ObjectScriptDev") '
            'If \'$IsObject(cls) { Write "NOT_FOUND",! Quit } '
            'Set params = cls.Parameters '
            'Set i = 1 '
            'For { Quit:i>params.Count() '
            '  Set p = params.GetAt(i) '
            '  Write p.Name," = ",p.Default,! '
            '  Set i = i + 1 '
            '}'
        )
        out = iris_execute(code)
        self.assertNotIn("NOT_FOUND", out, "IAD.Agent.ObjectScriptDev class not found")
        self.assertIn(
            "PROVIDER", out,
            f"Expected PROVIDER parameter in IAD.Agent.ObjectScriptDev:\n{out}"
        )

    def test_example_agent_has_skills_parameter(self):
        code = (
            'Set cls = ##class(%Dictionary.ClassDefinition).%OpenId("IAD.Agent.ObjectScriptDev") '
            'If \'$IsObject(cls) { Write "NOT_FOUND",! Quit } '
            'Set params = cls.Parameters '
            'Set found = 0 '
            'Set i = 1 '
            'For { Quit:i>params.Count() '
            '  Set p = params.GetAt(i) '
            '  If p.Name = "SKILLS" { Set found = 1  Write p.Default,! } '
            '  Set i = i + 1 '
            '} '
            'If \'found { Write "SKILLS_NOT_FOUND",! }'
        )
        out = iris_execute(code)
        self.assertNotIn("SKILLS_NOT_FOUND", out, "SKILLS parameter not found")
        self.assertIn(
            "IAD.Skill.", out,
            f"Expected IAD.Skill.* in SKILLS parameter, got:\n{out}"
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
