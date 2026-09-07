#!/usr/bin/env python3
"""Canaries for the detectors in `antipatterns.py`.

A detector that never fires is indistinguishable from a clean tree, and the whole gate is
built on detectors reporting zero. Every detector here gets two samples: one carrying the
bug, so the detector is shown to fire, and one carrying the fix, so it is shown not to fire
on the correct shape. The real tree is then asserted clean through the same code path, which
is what lets `undeclared-params` and `prose-only-enum` stay off the baseline.

The two detectors under test take a `{path: text}` mapping rather than reading the tree, so a
sample is a dict literal — no fixture files, no temp directories, and the sample sits next to
the assertion that explains it.

Run: scripts/gates/test_antipatterns.py     (exit 0 = all canaries pass)
"""

from __future__ import annotations

import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import antipatterns as ap  # noqa: E402  (path has to be set before the import)

FAILURES: list[str] = []


def check(label: str, condition: bool, detail: str = "") -> None:
    if condition:
        print(f"  ok   {label}")
        return
    FAILURES.append(f"{label}{': ' + detail if detail else ''}")
    print(f"  FAIL {label}{': ' + detail if detail else ''}")


def messages(findings) -> str:
    return " | ".join(f"{f.location} {f.message}" for f in findings)


# ---------------------------------------------------------------------------
# undeclared-params
# ---------------------------------------------------------------------------

OPEN_STRUCT = """
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StreamInspectParams {
    pub oid: String,
}

impl Tools {
    #[tool(description = "Inspect a stream.")]
    async fn stream_inspect(&self, Parameters(p): Parameters<StreamInspectParams>) {}
}
"""

CLOSED_STRUCT = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StreamInspectParams {
    pub oid: String,
}

impl Tools {
    #[tool(description = "Inspect a stream.")]
    async fn stream_inspect(&self, Parameters(p): Parameters<StreamInspectParams>) {}
}
"""

UNTYPED_REFERENT = """
impl Tools {
    #[tool(description = "Inspect a stream.")]
    async fn stream_inspect(&self, Parameters(p): Parameters<serde_json::Value>) {}
}
"""

FLATTENED_STRUCT = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StreamInspectParams {
    pub oid: String,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl Tools {
    #[tool(description = "Inspect a stream.")]
    async fn stream_inspect(&self, Parameters(p): Parameters<StreamInspectParams>) {}
}
"""

# The shape the detector must not report: a `…Params` bundle a dispatcher fills in Rust. Its
# keys never come from a client, so `deny_unknown_fields` would constrain nothing.
INTERNAL_BUNDLE = """
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProductionStatusParams {
    pub namespace: String,
}
"""


def test_undeclared_params() -> None:
    print("undeclared-params")

    found = ap.undeclared_params_findings({"src/a.rs": OPEN_STRUCT})
    check(
        "fires on a params struct without deny_unknown_fields",
        len(found) == 1 and "StreamInspectParams" in found[0].message,
        messages(found),
    )

    found = ap.undeclared_params_findings({"src/a.rs": CLOSED_STRUCT})
    check("silent once the attribute is there", not found, messages(found))

    found = ap.undeclared_params_findings({"src/a.rs": UNTYPED_REFERENT})
    check(
        "fires on Parameters<serde_json::Value>",
        len(found) == 1 and "Parameters<Value>" in found[0].message,
        messages(found),
    )

    found = ap.undeclared_params_findings({"src/a.rs": FLATTENED_STRUCT})
    check(
        "fires on a flatten into a Map, which the first two rules miss",
        len(found) == 1 and "rest" in found[0].message,
        messages(found),
    )

    found = ap.undeclared_params_findings({"src/a.rs": INTERNAL_BUNDLE})
    check(
        "leaves an internal argument bundle alone",
        not found,
        messages(found),
    )

    # A comment quoting the bad shape is the fix being documented, not the bug.
    found = ap.undeclared_params_findings(
        {"src/a.rs": "/// Was `Parameters<AnyParams>` until 113.\n" + CLOSED_STRUCT}
    )
    check("does not read its own explanation as a finding", not found, messages(found))

    found = ap.check_undeclared_params()
    check("reports zero on the real tree", not found, messages(found))


# ---------------------------------------------------------------------------
# prose-only-enum
# ---------------------------------------------------------------------------

PROSE_PIPES = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisSystemPerformanceParams {
    #[serde(default)]
    pub mode: Option<String>,
}

impl Tools {
    #[tool(description = "Profile an instance. mode: start | status | last_runid.")]
    async fn iris_system_performance(
        &self,
        Parameters(p): Parameters<IrisSystemPerformanceParams>,
    ) {
    }
}
"""

PROSE_PIPES_WITH_ENUM = PROSE_PIPES.replace(
    "    pub mode: Option<String>,",
    '    #[schemars(extend("enum" = ["start", "status", "last_runid"]))]\n'
    "    pub mode: Option<String>,",
)

PROSE_ONE_OF = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisSystemPerformanceParams {
    #[serde(default)]
    pub profile: Option<String>,
}

impl Tools {
    #[tool(
        description = "Profile an instance. profile: one of test, 30mins, 4hours, 24hours."
    )]
    async fn iris_system_performance(
        &self,
        Parameters(p): Parameters<IrisSystemPerformanceParams>,
    ) {
    }
}
"""

PROSE_ONE_OF_EXEMPT = PROSE_ONE_OF.replace(
    "    #[serde(default)]\n    pub profile",
    "    /// An instance may define its own profile, which is why this is not an enum.\n"
    "    #[serde(default)]\n    pub profile",
)

# Commas in a sentence are not a value set. If this fires, the pattern is too loose and the
# detector will be turned off by whoever it wakes up first.
PROSE_ENGLISH = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisQueryParams {
    #[serde(default)]
    pub query: Option<String>,
}

impl Tools {
    #[tool(
        description = "Run SQL. The query runs against the connection namespace, returns rows, \
                       and stops at the first error, so check the response."
    )]
    async fn iris_query(&self, Parameters(p): Parameters<IrisQueryParams>) {}
}
"""


def test_prose_only_enum() -> None:
    print("prose-only-enum")

    found = ap.prose_only_enum_findings({"src/a.rs": PROSE_PIPES})
    check(
        "fires on a pipe-separated value list with no enum",
        len(found) == 1 and "`mode`" in found[0].message,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": PROSE_PIPES_WITH_ENUM})
    check("silent once the parameter advertises the set", not found, messages(found))

    found = ap.prose_only_enum_findings({"src/a.rs": PROSE_ONE_OF})
    check(
        'fires on a comma list introduced by "one of"',
        len(found) == 1 and "`profile`" in found[0].message,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": PROSE_ONE_OF_EXEMPT})
    check(
        'silent when the doc comment says why this is "not an enum"',
        not found,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": PROSE_ENGLISH})
    check(
        "does not read an English sentence as a value set", not found, messages(found)
    )

    found = ap.check_prose_only_enum()
    check("reports zero on the real tree", not found, messages(found))


# ---------------------------------------------------------------------------
# empty-tests: delegation to a shared assert_* helper
# ---------------------------------------------------------------------------

ASSERTING_HELPER = """
pub fn assert_advertised_contracts(contracts: &[ParamContract]) {
    for contract in contracts {
        assert_eq!(read_keys(contract.tool), contract.names());
    }
}
"""

HOLLOW_HELPER = """
pub fn assert_advertised_contracts(_contracts: &[ParamContract]) {}
"""

DELEGATING_TEST = """
#[test]
fn batch1_handlers_read_their_contract() {
    assert_advertised_contracts(BATCH1);
}
"""

SILENT_TEST = """
#[test]
fn batch1_handlers_read_their_contract() {
    let _ = read_keys("stream_inspect");
}
"""


def test_empty_tests() -> None:
    print("empty-tests")

    found = ap.empty_tests_findings(
        {"src/testing.rs": ASSERTING_HELPER, "tests/b1.rs": DELEGATING_TEST}
    )
    check(
        "silent on a test delegating to an assert_* helper that asserts",
        not found,
        messages(found),
    )

    # The reason the helper's own body is checked: naming a function `assert_` is not
    # evidence that it asserts, and fourteen tests calling one hollow helper is fourteen
    # green lines over nothing.
    found = ap.empty_tests_findings(
        {"src/testing.rs": HOLLOW_HELPER, "tests/b1.rs": DELEGATING_TEST}
    )
    check(
        "fires when the assert_* helper it calls asserts nothing itself",
        len(found) == 1 and "batch1_handlers_read_their_contract" in found[0].message,
        messages(found),
    )

    found = ap.empty_tests_findings({"tests/b1.rs": SILENT_TEST})
    check(
        "still fires on a body with no assertion and no helper call",
        len(found) == 1,
        messages(found),
    )


# ---------------------------------------------------------------------------
# device-capture: a routine named in prose is not a call
# ---------------------------------------------------------------------------

DEVICE_CALL = """
const START: &str = r#"
    Do run^SystemPerformance("test")
    Write !,"started"
"#;
"""

DEVICE_CALL_GUARDED = """
const START: &str = r#"
    Set tIO=$IO
    Do run^SystemPerformance("test")
    Use tIO
    Write !,"started"
"#;
"""

DEVICE_IN_PROSE = """
/// `mode=status`: the run to poll. `waittime^SystemPerformance` answers an unknown ID with
/// `-2^no such runid`.
pub run_id: Option<String>,
"""


def test_device_capture() -> None:
    print("device-capture")

    found = ap.device_capture_findings({"crates/c/src/a.rs": DEVICE_CALL})
    check(
        "fires on an unguarded ^SystemPerformance call",
        len(found) == 1,
        messages(found),
    )

    found = ap.device_capture_findings({"crates/c/src/a.rs": DEVICE_CALL_GUARDED})
    check("silent once $IO is snapshotted", not found, messages(found))

    found = ap.device_capture_findings({"crates/c/src/a.rs": DEVICE_IN_PROSE})
    check(
        "does not read a doc comment naming the routine as a call",
        not found,
        messages(found),
    )


# ---------------------------------------------------------------------------
# The harness itself
# ---------------------------------------------------------------------------


def test_both_classes_are_never_baselined() -> None:
    print("registration")
    for name in ("undeclared-params", "prose-only-enum"):
        check(f"{name} is registered in CHECKS", name in ap.CHECKS)
        check(f"{name} is never baselined", name in ap.NO_BASELINE)
    baseline = ap.load_baseline()
    listed = sorted(k for k in baseline if k.split("\t", 1)[0] in ap.NO_BASELINE)
    check(
        "neither class has a line in antipatterns-baseline.txt",
        not listed,
        str(listed),
    )


def main() -> int:
    test_undeclared_params()
    test_prose_only_enum()
    test_empty_tests()
    test_device_capture()
    test_both_classes_are_never_baselined()
    if FAILURES:
        print(f"\n{len(FAILURES)} canary failure(s):")
        for f in FAILURES:
            print(f"  - {f}")
        return 1
    print("\nantipatterns canaries: all pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
