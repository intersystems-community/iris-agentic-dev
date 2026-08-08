"""
Tests for IAD.ToolSet.IrisAgenticDev and IAD.ToolSet.IrisAgenticDevReadOnly.

T-076-01: Import IAD.ToolSet.xml — zero compile errors.
T-076-02: %Discover() returns >=20 tools including iris_execute, iris_doc, check_config.
T-076-03: ReadOnly %Discover() excludes iris_compile, iris_execute, iris_source_control.
T-076-04: Agent round-trip — call check_config via an %AI.Agent with the full toolset.

Requirements:
  - IRIS AI Hub instance at aihub-iris-116 (port 21972)
  - IAD.ToolSet.xml must exist at contrib/aihub/IAD.ToolSet.xml
  - iris-agentic-dev binary on PATH or IRIS_AGENTIC_DEV_BIN set
  - ANTHROPIC_API_KEY set (T-076-04 only)
"""

import json
import os
import subprocess
import sys
import unittest

IRIS_HOST = os.environ.get("AIHUB_IRIS_HOST", "localhost")
IRIS_PORT = os.environ.get("AIHUB_IRIS_PORT", "21972")
IRIS_WEB_PORT = os.environ.get("AIHUB_IRIS_WEB_PORT", "21972")
IRIS_USERNAME = os.environ.get("AIHUB_IRIS_USERNAME", "_SYSTEM")
IRIS_PASSWORD = os.environ.get("AIHUB_IRIS_PASSWORD", "SYS")
IRIS_NAMESPACE = os.environ.get("AIHUB_IRIS_NAMESPACE", "USER")

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
XML_PATH = os.path.join(REPO_ROOT, "contrib", "aihub", "IAD.ToolSet.xml")
IAD_BIN = os.environ.get("IRIS_AGENTIC_DEV_BIN", "iris-agentic-dev")


def iad(*args, namespace=IRIS_NAMESPACE):
    """Run an iris-agentic-dev command against the AI Hub container."""
    cmd = [
        IAD_BIN,
        "--host", IRIS_HOST,
        "--web-port", IRIS_WEB_PORT,
        "--username", IRIS_USERNAME,
        "--password", IRIS_PASSWORD,
        "--namespace", namespace,
    ] + list(args)
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    return result


def iris_execute(code, namespace=IRIS_NAMESPACE):
    """Execute ObjectScript and return the tool output as a string."""
    result = iad("tool", "iris_execute", "--args", json.dumps({"code": code}), namespace=namespace)
    return result.stdout + result.stderr


class TestToolSetImport(unittest.TestCase):
    """T-076-01: Import smoke test."""

    def test_xml_exists(self):
        self.assertTrue(
            os.path.exists(XML_PATH),
            f"IAD.ToolSet.xml not found at {XML_PATH} — run Phase 3 (export) first"
        )

    def test_import_zero_errors(self):
        """Import the XML into IRIS and verify no compile errors."""
        if not os.path.exists(XML_PATH):
            self.skipTest("IAD.ToolSet.xml not yet created")

        # Copy to a path IRIS can read (inside the container or accessible path)
        code = (
            f'Set sc = $system.OBJ.Load("{XML_PATH}", "ck") '
            f'If $$$ISERR(sc) {{ Write $system.Status.GetErrorText(sc),! }} '
            f'Else {{ Write "import_ok",! }}'
        )
        out = iris_execute(code)
        self.assertIn(
            "import_ok", out,
            f"Import failed or produced errors:\n{out}"
        )
        self.assertNotIn("ERROR", out.upper().replace("import_ok", ""),
                         f"Compile errors found:\n{out}")

    def test_both_classes_compiled(self):
        """Verify both ToolSet classes compiled successfully."""
        if not os.path.exists(XML_PATH):
            self.skipTest("IAD.ToolSet.xml not yet created")

        for cls in ("IAD.ToolSet.IrisAgenticDev", "IAD.ToolSet.IrisAgenticDevReadOnly"):
            code = f'Write $system.OBJ.IsCompiled("{cls}"),!'
            out = iris_execute(code)
            self.assertIn(
                "1", out,
                f"{cls} is not compiled — import may have failed:\n{out}"
            )


class TestToolSetDiscovery(unittest.TestCase):
    """T-076-02: Full toolset discovers >=20 tools including expected names."""

    EXPECTED_TOOLS = {"iris_execute", "iris_doc", "iris_query", "check_config", "iris_compile"}

    def test_full_toolset_min_count(self):
        code = (
            "Set ts = ##class(IAD.ToolSet.IrisAgenticDev).%New() "
            "Set tools = ts.%Discover() "
            "Write tools.Count(),!"
        )
        out = iris_execute(code)
        # Extract the count (first integer on a line)
        count = None
        for line in out.splitlines():
            line = line.strip()
            if line.isdigit():
                count = int(line)
                break
        self.assertIsNotNone(count, f"Could not parse tool count from output:\n{out}")
        self.assertGreaterEqual(
            count, 20,
            f"Expected >=20 tools, got {count}:\n{out}"
        )

    def test_full_toolset_contains_expected_tools(self):
        code = (
            "Set ts = ##class(IAD.ToolSet.IrisAgenticDev).%New() "
            "Set tools = ts.%Discover() "
            "Set i = 1 "
            "For { Quit:i>tools.Count() "
            "  Write tools.GetAt(i).name,\",\" "
            "  Set i = i + 1 "
            "}"
        )
        out = iris_execute(code)
        tool_names = {t.strip() for t in out.replace("\n", ",").split(",") if t.strip()}
        for expected in self.EXPECTED_TOOLS:
            self.assertIn(
                expected, tool_names,
                f"Expected tool '{expected}' missing from toolset. Found: {sorted(tool_names)}"
            )


class TestReadOnlyToolSet(unittest.TestCase):
    """T-076-03: Read-only variant excludes write tools."""

    EXCLUDED_TOOLS = {"iris_compile", "iris_execute", "iris_source_control"}

    def test_readonly_excludes_write_tools(self):
        code = (
            "Set ts = ##class(IAD.ToolSet.IrisAgenticDevReadOnly).%New() "
            "Set tools = ts.%Discover() "
            "Set i = 1 "
            "For { Quit:i>tools.Count() "
            "  Write tools.GetAt(i).name,\",\" "
            "  Set i = i + 1 "
            "}"
        )
        out = iris_execute(code)
        tool_names = {t.strip() for t in out.replace("\n", ",").split(",") if t.strip()}
        for excluded in self.EXCLUDED_TOOLS:
            self.assertNotIn(
                excluded, tool_names,
                f"Write tool '{excluded}' should be excluded from ReadOnly toolset but was found"
            )

    def test_readonly_still_has_read_tools(self):
        code = (
            "Set ts = ##class(IAD.ToolSet.IrisAgenticDevReadOnly).%New() "
            "Set tools = ts.%Discover() "
            "Set i = 1 "
            "For { Quit:i>tools.Count() "
            "  Write tools.GetAt(i).name,\",\" "
            "  Set i = i + 1 "
            "}"
        )
        out = iris_execute(code)
        tool_names = {t.strip() for t in out.replace("\n", ",").split(",") if t.strip()}
        self.assertIn(
            "iris_query", tool_names,
            f"Read tool 'iris_query' should be present in ReadOnly toolset"
        )
        self.assertIn(
            "check_config", tool_names,
            f"Read tool 'check_config' should be present in ReadOnly toolset"
        )


class TestAgentRoundTrip(unittest.TestCase):
    """T-076-04: Agent round-trip — call check_config via %AI.Agent."""

    def setUp(self):
        if not os.environ.get("ANTHROPIC_API_KEY"):
            self.skipTest("ANTHROPIC_API_KEY not set — skipping live agent test")

    def test_agent_check_config(self):
        code = (
            'Set agent = ##class(%AI.Agent).%New() '
            'Set agent.Provider = "anthropic" '
            'Set agent.ApiKey = $system.Util.GetEnviron("ANTHROPIC_API_KEY") '
            'Do agent.UseToolSet("IAD.ToolSet.IrisAgenticDev") '
            'Set result = agent.Run("Call check_config and return the raw JSON result") '
            'Write result,!'
        )
        out = iris_execute(code)
        self.assertTrue(
            "connected" in out.lower() or "connection_source" in out.lower() or "host" in out.lower(),
            f"Expected check_config response in agent output, got:\n{out[:800]}"
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
