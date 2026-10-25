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
# prose-only-enum: the shapes the first two patterns never saw
#
# The detector reported zero on eleven dispatchers whose value sets are spelled out in prose,
# because none of them writes `mode: a | b | c` or "one of a, b, c". Three more shapes, taken
# verbatim from the tools that were missed:
#
#   iris_info          what=documents lists all docs, what=modified lists recently changed, …
#   iris_global        action: get=read a node, set=write a node, kill=delete, list=enumerate
#   iris_doc           mode: get (fetch source), put (write, auto SCM checkout), delete, head …
#
# plus two ways of being invisible rather than unmatched: a description written with backslash
# line continuations (`iris_admin`, the worst offender at 25 actions, parsed as no tool at all),
# and a dispatcher that documents nothing and only branches (`iris_admin` again — the prose lists
# its actions under "Read actions:", not under the parameter name).
# ---------------------------------------------------------------------------

PROSE_ASSIGNED = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoParams {
    #[serde(default)]
    pub what: Option<String>,
}

impl Tools {
    #[tool(
        description = "Discover IRIS namespace contents. what=documents lists all docs, \
        what=modified lists recently changed, what=namespace returns config."
    )]
    async fn iris_info(&self, Parameters(p): Parameters<InfoParams>) {}
}
"""

PROSE_COLON_GLOSS = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisGlobalParams {
    pub action: String,
}

impl Tools {
    #[tool(
        description = "Read, write, kill, or list IRIS global nodes. action: get=read a node or \
        subtree, set=write a node, kill=delete a node/subtree, list=enumerate subscripts."
    )]
    async fn iris_global(&self, Parameters(p): Parameters<IrisGlobalParams>) {}
}
"""

PROSE_BARE_COMMA = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisDocParams {
    pub mode: String,
}

impl Tools {
    #[tool(
        description = "Read/write/delete IRIS documents. mode: get (fetch source), put (write, \
        auto SCM checkout), delete, head (existence), list (glob `pattern`)."
    )]
    async fn iris_doc(&self, Parameters(p): Parameters<IrisDocParams>) {}
}
"""

# The set is real but the parameter is exempt, written the way `profile` writes it. The exemption
# has to survive every new shape, or the first legitimately open value set turns the gate off.
PROSE_BARE_COMMA_EXEMPT = PROSE_BARE_COMMA.replace(
    "    pub mode: String,",
    "    /// A site may register its own mode, so this is not an enum.\n"
    "    pub mode: String,",
)

# A dispatcher that documents nothing and only branches. Nothing in the description names a value,
# so every prose pattern is silent by construction; the arms are the contract.
MATCH_ARM_ONLY = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisAdminParams {
    pub action: String,
}

impl Tools {
    #[tool(description = "IRIS administration dispatcher.")]
    async fn iris_admin(&self, Parameters(p): Parameters<IrisAdminParams>) {
        let action = p.action.as_str();
        let result = match action {
            "list_namespaces" => admin::list_namespaces(iris).await,
            "list_users" => admin::list_users(iris).await,
            "create_user" => admin::create_user(iris).await,
            _ => return err_result(unknown_action(action)),
        };
        result
    }
}
"""

MATCH_ARM_WITH_ENUM = MATCH_ARM_ONLY.replace(
    "    pub action: String,",
    '    #[schemars(extend("enum" = ["list_namespaces", "list_users", "create_user"]))]\n'
    "    pub action: String,",
)

# A `match` on something that is not a parameter. `p.action` is in the struct; a connection state
# machine is not, and a detector that reads every `match` in the body would report it.
MATCH_ARM_NOT_A_PARAM = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisQueryParams {
    pub query: String,
}

impl Tools {
    #[tool(description = "Run SQL.")]
    async fn iris_query(&self, Parameters(p): Parameters<IrisQueryParams>) {
        let transport = match self.connection_kind() {
            "atelier" => Transport::Rest,
            "docker" => Transport::Exec,
            _ => Transport::None,
        };
        transport
    }
}
"""


# The set is in the field's own doc comment, not in the tool description, and it is two values
# joined by "or" — `kb` and `skill_community` both write it this way. Two values is still a set a
# client can validate against, and the doc comment is still prose the optimizer may rewrite.
DOC_COMMENT_OR = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KbParams {
    /// Action: index or recall
    pub action: String,
}

impl Tools {
    #[tool(description = "Knowledge base tools.")]
    async fn kb(&self, Parameters(p): Parameters<KbParams>) {}
}
"""

DOC_COMMENT_OR_WITH_ENUM = DOC_COMMENT_OR.replace(
    "    pub action: String,",
    '    #[schemars(extend("enum" = ["index", "recall"]))]\n    pub action: String,',
)

# An ordinary "either/or" sentence in a doc comment is not a value set. If this fires, every
# optional parameter in the tree becomes a finding.
DOC_COMMENT_PROSE = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisQueryParams {
    /// SQL to run. Rows come back as objects, so read the response or the count.
    pub query: String,
}

impl Tools {
    #[tool(description = "Run SQL.")]
    async fn iris_query(&self, Parameters(p): Parameters<IrisQueryParams>) {}
}
"""


# A field whose wire name is a Rust keyword is written `r#type`. The field pattern read the name
# as starting at `[a-z_]` and running to the colon, so `r#type` matched nothing and the parameter
# was not in the struct's index at all — the same invisibility as the unparsed description, one
# level down. `iris_admin.type` is the parameter this hid.
RAW_IDENT_FIELD = """
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisAdminParams {
    /// `list_webapps`: keep only applications of this type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
}

impl Tools {
    #[tool(description = "IRIS administration dispatcher. type: REST | CSP.")]
    async fn iris_admin(&self, Parameters(p): Parameters<IrisAdminParams>) {}
}
"""

RAW_IDENT_FIELD_WITH_ENUM = RAW_IDENT_FIELD.replace(
    "    pub r#type: Option<String>,",
    '    #[schemars(extend("enum" = ["REST", "CSP"]))]\n    pub r#type: Option<String>,',
)


def test_prose_only_enum_unseen_shapes() -> None:
    print("prose-only-enum: unseen shapes")

    found = ap.prose_only_enum_findings({"src/a.rs": PROSE_ASSIGNED})
    check(
        "fires on repeated `what=value` mentions (iris_info)",
        len(found) == 1 and "`what`" in found[0].message,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": PROSE_COLON_GLOSS})
    check(
        "fires on `action: get=…, set=…, kill=…` (iris_global)",
        len(found) == 1 and "`action`" in found[0].message,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": PROSE_BARE_COMMA})
    check(
        "fires on a bare comma list with no lead-in (iris_doc)",
        len(found) == 1 and "`mode`" in found[0].message,
        messages(found),
    )
    check(
        "a parenthetical gloss does not become a value",
        found and "fetch source" not in found[0].message,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": PROSE_BARE_COMMA_EXEMPT})
    check(
        'the "not an enum" exemption still silences the new shapes',
        not found,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": MATCH_ARM_ONLY})
    check(
        "fires on a dispatcher that only branches, with no value in the prose",
        len(found) == 1 and "`action`" in found[0].message,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": MATCH_ARM_WITH_ENUM})
    check(
        "silent once the branching parameter advertises its arms",
        not found,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": MATCH_ARM_NOT_A_PARAM})
    check(
        "leaves a match on something that is not a parameter alone",
        not found,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": DOC_COMMENT_OR})
    check(
        'fires on a two-value set written "index or recall" in the field doc comment',
        len(found) == 1 and "`action`" in found[0].message,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": DOC_COMMENT_OR_WITH_ENUM})
    check(
        "silent once that two-value set is declared",
        not found,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": DOC_COMMENT_PROSE})
    check(
        'does not read an "or" in an ordinary doc comment as a value set',
        not found,
        messages(found),
    )

    # `iris_admin` writes its description with backslash line continuations, and the description
    # pattern had no re.S, so the whole tool parsed as absent — not clean, invisible.
    found = ap.prose_only_enum_findings(
        {
            "src/a.rs": PROSE_PIPES.replace(
                'description = "Profile an instance. mode: start | status | last_runid."',
                'description = "Profile an instance. \\\n        mode: start | status | last_runid."',
            )
        }
    )
    check(
        "reads a description written with backslash line continuations",
        len(found) == 1 and "`mode`" in found[0].message,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": RAW_IDENT_FIELD})
    check(
        "reads a raw-identifier field (`r#type`) as the parameter `type`",
        len(found) == 1 and "`type`" in found[0].message,
        messages(found),
    )

    found = ap.prose_only_enum_findings({"src/a.rs": RAW_IDENT_FIELD_WITH_ENUM})
    check(
        "silent once the raw-identifier field advertises its set",
        not found,
        messages(found),
    )


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

# A fixture that is *about* an unterminated block comment has to contain one. The comment
# stripper read the `/*` inside this string literal as the start of a real comment, found no
# `*/`, and swallowed the assertion and every line after it in the file — so the test that
# proves the gate is not blinded by an unterminated comment was itself reported as asserting
# nothing (#135's own test, flagged in CI run 34163864517).
STRING_HELD_COMMENT_OPENER = """
#[test]
fn an_unterminated_block_comment_does_not_blind_the_gate() {
    let cls = format!("Class My.Sneaky\\n/* opened and never closed\\n{REAL_GENERATOR}");
    assert!(check_compile_time_code_mode(&cls, "My.Sneaky.cls").is_some());
}

#[test]
fn a_later_test_in_the_same_file_still_gets_read() {
    assert!(check_compile_time_code_mode("Class A {}", "A.cls").is_none());
}
"""

STRING_HELD_LINE_COMMENT = """
#[test]
fn a_double_slash_in_a_literal_is_not_a_comment() {
    let url = "https://host:52780/api/atelier/"; assert!(url.starts_with("https"));
}
"""


def test_comment_masking_skips_string_literals() -> None:
    print("comment masking")

    code = 'let a = "/* not a comment";\nassert!(a.len() > 0);\n'
    check(
        "strip_comments keeps code after a `/*` held in a string literal",
        "assert!" in ap.strip_comments(code),
        ap.strip_comments(code),
    )
    check(
        "mask_comments keeps code after a `/*` held in a string literal",
        "assert!" in ap.mask_comments(code),
        ap.mask_comments(code),
    )

    masked = ap.mask_comments(code)
    check(
        "mask_comments preserves length, so reported line numbers stay right",
        len(masked) == len(code),
        f"{len(masked)} vs {len(code)}",
    )

    commented = 'let a = 1; /* real "quoted" comment */ assert!(a == 1);\n'
    check(
        "a quote inside a real comment does not start a literal",
        'real "quoted" comment' not in ap.mask_comments(commented),
        ap.mask_comments(commented),
    )

    raw = 'let a = r#"/* inside a raw string "#; assert!(a.len() > 0);\n'
    check(
        "a raw string literal hides a `/*` too",
        "assert!" in ap.mask_comments(raw),
        ap.mask_comments(raw),
    )

    escaped = 'let a = "ends with a backslash \\\\"; /* c */ assert!(true);\n'
    check(
        "an escaped backslash ends the literal it belongs to",
        "assert!" in ap.mask_comments(escaped)
        and "/* c */" not in ap.mask_comments(escaped),
        ap.mask_comments(escaped),
    )


def test_empty_tests() -> None:
    print("empty-tests")

    found = ap.empty_tests_findings({"tests/b1.rs": STRING_HELD_COMMENT_OPENER})
    check(
        "silent on a test whose fixture string holds an unterminated `/*`",
        not found,
        messages(found),
    )

    found = ap.empty_tests_findings({"tests/b1.rs": STRING_HELD_LINE_COMMENT})
    check(
        "silent on a test whose fixture string holds a `//`",
        not found,
        messages(found),
    )

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
# scored-exception
# ---------------------------------------------------------------------------

# The shape that shipped: an unreachable scorer answers with a number on the scale, so the
# report cannot tell "the agent failed every task" from "nothing was ever scored".
SCORE_FROM_HANDLER = """
def score_result(task, result):
    try:
        parsed = json.loads(client.messages.create(**kw).content[0].text)
        return {"score": int(parsed["score"]), "reasoning": parsed.get("reasoning", "")}
    except Exception as e:
        return {"score": 0, "reasoning": f"Judge error: {e}"}
"""

SCORE_FROM_HANDLER_FIXED = """
def score_result(task, result):
    try:
        parsed = json.loads(client.messages.create(**kw).content[0].text)
        return {"scored": True, "score": int(parsed["score"]), "reasoning": ""}
    except Exception as e:
        return {"scored": False, "score": None, "reasoning": f"scorer unreachable: {e}"}
"""

# `dict(score=0)` is the same fabrication with different punctuation. A detector that only
# knows the brace form gets defeated by whoever reaches for the other one.
SCORE_FROM_HANDLER_DICT_CALL = """
def score_result(task, result):
    try:
        return dict(score=parsed["score"], reasoning="")
    except Exception as e:
        return dict(score=0, reasoning=str(e))
"""

SCORE_FROM_HANDLER_FLOAT = """
def score_result(task, result):
    try:
        return {"score": parsed["score"]}
    except Exception as e:
        return {"score": 0.0, "reasoning": str(e)}
"""

# A handler that gives up honestly. Nothing lands on the scale, so nothing to report.
HANDLER_RERAISES = """
def score_result(task, result):
    try:
        return {"score": parsed["score"]}
    except KeyError as e:
        raise ValueError(f"malformed scorer response: {e}") from e
"""

# The near-miss: a test naming the zero it expects, and a fixture handing one back. Both sit
# outside any handler, and both are legitimate — a real zero is a real verdict.
ZERO_OUTSIDE_A_HANDLER = """
def a_failing_verdict():
    return {"score": 0, "reasoning": "the agent never called a tool"}


def test_a_bad_transcript_scores_zero():
    assert score_result(task, transcript) == {"score": 0, "reasoning": "no tool call"}
"""


def test_scored_exception() -> None:
    print("scored-exception")

    found = ap.scored_exception_findings(
        {"benchmark/021/runner/judge.py": SCORE_FROM_HANDLER}
    )
    check(
        "fires on a score returned from an except handler",
        len(found) == 1 and "`score_result`" in found[0].message,
        messages(found),
    )

    found = ap.scored_exception_findings({"a.py": SCORE_FROM_HANDLER_FIXED})
    check(
        "silent once the handler returns the unscored verdict",
        not found,
        messages(found),
    )

    found = ap.scored_exception_findings({"a.py": SCORE_FROM_HANDLER_DICT_CALL})
    check("fires on the dict(score=0) spelling", len(found) == 1, messages(found))

    found = ap.scored_exception_findings({"a.py": SCORE_FROM_HANDLER_FLOAT})
    check(
        "fires on a float score, which is out of range as well as fabricated",
        len(found) == 1 and "0.0" in found[0].message,
        messages(found),
    )

    found = ap.scored_exception_findings({"a.py": HANDLER_RERAISES})
    check("leaves a handler that re-raises alone", not found, messages(found))

    found = ap.scored_exception_findings({"a.py": ZERO_OUTSIDE_A_HANDLER})
    check(
        "does not read a real zero verdict outside a handler as a finding",
        not found,
        messages(found),
    )

    # A file the scanner cannot parse is a file the scanner has not cleared, and saying so is
    # the only honest outcome — the alternative is a silent skip that reports clean.
    found = ap.scored_exception_findings({"a.py": "def broken(:\n    pass\n"})
    check(
        "reports a file it cannot parse rather than skipping it",
        len(found) == 1 and "does not parse" in found[0].message,
        messages(found),
    )

    # This canary read "finds the known instance in the real tree" until 118 T013 rewrote
    # `judge.py`'s failure path to return an unscored verdict. The class is now clean, which is
    # what lets `scored-exception` be never-baselined: any finding here is new.
    found = ap.check_scored_exception()
    check(
        "reports zero on the real tree",
        not found,
        messages(found),
    )


# ---------------------------------------------------------------------------
# unpowered-result — 121 T015
# ---------------------------------------------------------------------------

# The shipped instance, `lift.py:76`: two point estimates subtracted, and whatever prints the
# result has no item count and no resolution to print beside it.
LIFT_FROM_TWO_POINT_ESTIMATES = """
def compute_lift_from_scores(baseline, skill):
    return {"lift": skill.pass_rate - baseline.pass_rate}
"""

# The fix: the lift comes off a paired comparison, and the dict carries what measured it.
LIFT_FROM_A_COMPARISON = """
def compute_lift_from_scores(baseline, skill):
    comparison = pair_arms("baseline", "skill", baseline, skill)
    return {"lift": comparison.lift, "comparison": comparison.to_dict()}
"""

# A formatting call site that prints a lift and nothing that says how well it was measured.
LIFT_PRINTED_BARE = """
def print_summary(run):
    for r in run.skills:
        print(f"{r.skill:<30}{r.lift:+.2f}  {r.outcome}")
"""

# The same call site once the item count and the MDE travel with the number.
LIFT_PRINTED_WITH_ITS_EVIDENCE = """
def print_summary(run):
    for r in run.skills:
        lift, pairs, mde, _ = _lift_cells(r)
        print(f"{r.skill:<30}{lift:>7}{pairs:>7}{mde:>6}  {r.outcome}")
"""

# A serialized result is a published figure too, and the same rule applies to it.
LIFT_SERIALIZED_BARE = """
def entry(result):
    return {"skill": result.skill, "lift": result.lift, "fire_rate": result.fire_rate}
"""

# Two rates printed side by side are not a lift. The detector must not invent one.
RATES_WITHOUT_A_LIFT = """
def print_arms(run):
    for r in run.skills:
        print(f"{r.skill:<30}{r.pass_rate_baseline:>6}{r.pass_rate_skill:>7}")
"""

# `nightly_canary.lift_mentions` — a scan *for* the word, so the nightly cannot publish one. It
# mentions a lift in a message and holds a word list called `_LIFT_WORDS`; it emits no number.
# The one function in the tree whose whole job is keeping lift out of the nightly was the one
# function the detector flagged, because a word appearing anywhere read as a figure published.
A_GUARD_THAT_SCANS_FOR_LIFT = '''
_LIFT_WORDS = ("lift", "pass_rate", "delta")


def lift_mentions(payload):
    """Every place a lift-ish word appears in the report, as `path: text`."""
    found = []

    def walk(node, path):
        if isinstance(node, dict):
            for key, value in node.items():
                here = f"{path}.{key}" if path else str(key)
                if any(word in str(key).lower() for word in _LIFT_WORDS):
                    found.append(f"{here}: key names a lift")
                walk(value, here)

    walk(payload, "")
    return found
'''


def test_unpowered_result() -> None:
    print("unpowered-result")

    found = ap.unpowered_result_findings(
        {"tests/e2e/skill_eval/lift.py": LIFT_FROM_TWO_POINT_ESTIMATES}
    )
    check(
        "fires on a lift subtracted out of two pass rates",
        len(found) >= 1 and any("point estimate" in f.message for f in found),
        messages(found),
    )

    found = ap.unpowered_result_findings({"a.py": LIFT_FROM_A_COMPARISON})
    check(
        "silent once the lift comes off a comparison that carries its item count",
        not found,
        messages(found),
    )

    found = ap.unpowered_result_findings({"a.py": LIFT_PRINTED_BARE})
    check(
        "fires on a formatting call site that omits the MDE",
        len(found) == 1 and "mde" in found[0].message.lower(),
        messages(found),
    )

    found = ap.unpowered_result_findings({"a.py": LIFT_PRINTED_WITH_ITS_EVIDENCE})
    check(
        "silent on a call site that prints the item count and the MDE beside the lift",
        not found,
        messages(found),
    )

    found = ap.unpowered_result_findings({"a.py": LIFT_SERIALIZED_BARE})
    check(
        "fires on a serialized lift with no comparison beside it",
        len(found) == 1,
        messages(found),
    )

    found = ap.unpowered_result_findings({"a.py": RATES_WITHOUT_A_LIFT})
    check(
        "leaves two pass rates printed side by side alone — that is not a lift",
        not found,
        messages(found),
    )

    found = ap.unpowered_result_findings({"a.py": A_GUARD_THAT_SCANS_FOR_LIFT})
    check(
        "leaves a guard that scans for the word `lift` alone — it names no number",
        not found,
        messages(found),
    )

    found = ap.unpowered_result_findings({"a.py": "def broken(:\n    pass\n"})
    check(
        "reports a file it cannot parse rather than skipping it",
        len(found) == 1 and "does not parse" in found[0].message,
        messages(found),
    )

    # The class is clean in the tree as of 121 Phase 1, which is what lets it be never-baselined.
    found = ap.check_unpowered_result()
    check("reports zero on the real tree", not found, messages(found))


# ---------------------------------------------------------------------------
# The harness itself
# ---------------------------------------------------------------------------


def test_both_classes_are_never_baselined() -> None:
    print("registration")
    for name in (
        "undeclared-params",
        "prose-only-enum",
        "scored-exception",
        "unpowered-result",
    ):
        check(f"{name} is registered in CHECKS", name in ap.CHECKS)
        check(f"{name} is never baselined", name in ap.NO_BASELINE)
    baseline = ap.load_baseline()
    listed = sorted(k for k in baseline if k.split("\t", 1)[0] in ap.NO_BASELINE)
    check(
        "neither class has a line in antipatterns-baseline.txt",
        not listed,
        str(listed),
    )


def test_baseline_key_ignores_line_numbers() -> None:
    """A baselined finding must survive code moving up or down its file.

    The old key was `check\tpath:line`, so editing anything above a finding reported it as new
    *and* stale at once. That taught the wrong habit: retarget the baseline line without reading
    what it says. Identity is now the check, the file, and a hash of the message.
    """
    print("baseline key")
    a = ap.Finding("empty-tests", "crates/x/src/a.rs:12", "`t` asserts nothing")
    moved = ap.Finding("empty-tests", "crates/x/src/a.rs:480", "`t` asserts nothing")
    other = ap.Finding("empty-tests", "crates/x/src/a.rs:12", "`u` asserts nothing")
    elsewhere = ap.Finding("empty-tests", "crates/x/src/b.rs:12", "`t` asserts nothing")

    check("same finding at a new line keeps its key", a.key() == moved.key())
    check("no line number survives in the key", ":12" not in a.key())
    check("a different message is a different key", a.key() != other.key())
    check(
        "the same message in another file is a different key",
        a.key() != elsewhere.key(),
    )
    check(
        "rewrapped whitespace keeps its key",
        a.key() == ap.Finding(a.check, a.location, "`t`   asserts\n  nothing").key(),
    )
    check(
        "the gist comment is not part of the key", a.baseline_line().startswith(a.key())
    )


def test_baseline_counts_repeats() -> None:
    """Two findings that agree on check, file, and message are two findings, not one.

    A set-based baseline could not say "this fires twice here", so the second copy rode in free on
    the first one's line. The real baseline had exactly one such pair when this was written.
    """
    print("baseline repeats")
    counts = ap.load_baseline()
    check("load_baseline counts occurrences", hasattr(counts, "most_common"))
    check(
        "the real baseline has at least one key firing more than once",
        any(n > 1 for n in counts.values()),
        "no repeated key — if the duplicates were genuinely fixed, delete this canary",
    )


def test_tool_name_refs() -> None:
    """The only detector here that reads the tree rather than a mapping, so it had no canary.

    It fires on any `iris_*` token a document names that the router does not answer to, which
    is right for `iris_document` (remediation text pointed at a tool that never existed) and
    wrong for two shapes a document legitimately contains: a response *field* named
    `iris_version`, and a deliberately-misspelled tool inside a quoted error message. The first
    was papered over with eleven baseline lines rather than named as a non-tool.
    """
    import check_tool_names as ctn

    known = ctn.registered_tools() | ctn.ALLOWED

    fires = ctn.scan_text(
        "docs/example.md", "Call `iris_document` to read a class.", known
    )
    check(
        "tool-name-refs fires on a tool that does not exist",
        [name for _, name in fires] == ["iris_document"],
        f"got {fires}",
    )

    field = ctn.scan_text(
        "docs/example.md", '  "iris_version": "IRIS for UNIX ... 2026.2",', known
    )
    check(
        "tool-name-refs ignores the iris_version response field",
        field == [],
        f"got {field}",
    )

    real = ctn.scan_text(
        "docs/example.md", "Run `iris_query` against the namespace.", known
    )
    check("tool-name-refs ignores a registered tool", real == [], f"got {real}")

    quoted = "error: unknown tool 'iris_quer'"
    check(
        "tool-name-refs fires on a misspelled tool in a file with no exemption",
        [name for _, name in ctn.scan_text("docs/example.md", quoted, known)]
        == ["iris_quer"],
    )
    check(
        "tool-name-refs respects a path-scoped exemption for that same name",
        ctn.scan_text("docs/getting-started.md", quoted, known) == [],
    )


# ---------------------------------------------------------------------------
# objectscript-splice: a placeholder inside a generated literal, or a backslash escape
# ---------------------------------------------------------------------------

SPLICE_QUOTED_RAW = """
fn a(dir: &str) -> String {
    format!(
        r#"Set dir="{dir}"
Write dir"#
    )
}
"""

SPLICE_QUOTED_NORMAL = """
fn b(name: &str) -> String {
    format!("Set n=\\"{name}\\" Write n")
}
"""

SPLICE_SQL = """
fn c(table: &str) -> String {
    format!("SELECT Name FROM %Dictionary.ClassDefinition WHERE Name = '{table}'")
}
"""

SPLICE_BACKSLASH = """
fn d(v: &str) -> String {
    let e = v.replace('"', "\\\\\\"");
    e
}
"""

SPLICE_CLEAN = """
fn e(dir: &str) -> String {
    let dir_expr = crate::objectscript::os_str_expr(dir);
    format!(r#"Set dir={dir_expr}
Write "{{done}}""#)
}
fn f(x: &str) -> String {
    format!("{x}")
}
const SOURCE: &str = "IADFixture.Svc";
fn f3() -> String {
    format!(r#"Set tHdr.SourceConfigName="{SOURCE}" Write 1"#)
}
fn f2(x: &str) -> String {
    let x_lit = crate::objectscript::os_lit_inner(x);
    format!(r#"Set v="{x_lit}" Write v"#)
}
/// Comment: `Set dir="{dir}"` explains the old shape.
fn g() {}
"""

SPLICE_MARKED = """
fn h(rule_q: &str) -> String {
    // splice-ok: rule_q has its single quotes doubled two lines up
    format!("SELECT ID FROM Ens_Rule.RuleSet WHERE Name = '{rule_q}'")
}
"""


def test_objectscript_splice() -> None:
    print("objectscript-splice")
    for label, text in [
        ("a placeholder inside a raw-string ObjectScript literal", SPLICE_QUOTED_RAW),
        ("a placeholder inside an escaped quote", SPLICE_QUOTED_NORMAL),
        ("a placeholder inside a SQL literal", SPLICE_SQL),
        ("a backslash escape ObjectScript does not have", SPLICE_BACKSLASH),
    ]:
        found = ap.objectscript_splice_findings({"crates/c/src/a.rs": text})
        check(f"fires on {label}", len(found) == 1, messages(found))
    found = ap.objectscript_splice_findings({"crates/c/src/a.rs": SPLICE_CLEAN})
    check(
        "silent on an os_str_expr splice, a _lit placeholder, a CONSTANT, escaped braces, a bare format and a comment",
        not found,
        messages(found),
    )
    found = ap.objectscript_splice_findings({"crates/c/src/a.rs": SPLICE_MARKED})
    check(
        "silent where a splice-ok comment gives the reason", not found, messages(found)
    )


# A positional `{}` or `{0}` placeholder names nothing, so the named-only pattern never saw it.
SPLICE_POSITIONAL_EMPTY = """
fn p1(a: &str, b: &str) -> String {
    format!(" Set {} = \\"{}\\"", a, b)
}
"""

SPLICE_POSITIONAL_INDEX = """
fn p2(a: &str) -> String {
    format!("Set v=\\"{0}\\" Write v", a)
}
"""

SPLICE_POSITIONAL_RAW = """
fn p3(a: &str) -> String {
    format!(r#"Set v="{}" Write v"#, a)
}
"""

SPLICE_POSITIONAL_SQL = """
fn p4(a: &str) -> String {
    format!("SELECT Name FROM Sample.Person WHERE Name = '{}'", a)
}
"""

SPLICE_POSITIONAL_MARKED = """
fn p5(ts: &str) -> String {
    // splice-ok: ts is a validated timestamp
    format!(" Set t = \\"{}\\"", ts)
}
"""

SPLICE_POSITIONAL_CLEAN = """
fn p6(a: &str) -> String {
    format!(" Set v = {}", crate::objectscript::os_str_expr(a))
}
fn p7(n: u32) -> String {
    format!("Write {}", n)
}
fn p8() -> String {
    format!(r#"Set v="{{}}" Write v"#)
}
"""


def test_objectscript_splice_positional() -> None:
    print("objectscript-splice (positional placeholders)")
    for label, text, want in [
        ("an empty {} inside an escaped quote", SPLICE_POSITIONAL_EMPTY, 1),
        ("a {0} inside an escaped quote", SPLICE_POSITIONAL_INDEX, 1),
        ("an empty {} inside a raw-string quote", SPLICE_POSITIONAL_RAW, 1),
        ("an empty {} inside a SQL literal", SPLICE_POSITIONAL_SQL, 1),
    ]:
        found = ap.objectscript_splice_findings({"crates/c/src/a.rs": text})
        check(f"fires on {label}", len(found) == want, messages(found))
    found = ap.objectscript_splice_findings(
        {"crates/c/src/a.rs": SPLICE_POSITIONAL_MARKED}
    )
    check(
        "silent on a positional splice with a splice-ok reason",
        not found,
        messages(found),
    )
    found = ap.objectscript_splice_findings(
        {"crates/c/src/a.rs": SPLICE_POSITIONAL_CLEAN}
    )
    check(
        "silent on an unquoted os_str_expr splice, a number and escaped braces",
        not found,
        messages(found),
    )


# committed-api-key: a 32-character hex key on a line that names Algolia or a search key.
# TPRM-6602: the docs site's Algolia key sat in four tracked files. The hex here is made up.
FAKE_HEX = "ab" * 16
KEY_FILES = {
    "crates/c/src/a.rs": f'const ALGOLIA_SEARCH_KEY: &str = "{FAKE_HEX}";\n',
    "skills/s/SKILL.md": f'  -H "X-Algolia-API-Key: {FAKE_HEX}" \\\n',
    "specs/s/spec.md": f"- SearchKey: `{FAKE_HEX}`\n",
}
KEY_CLEAN = {
    # A key read at run time, a placeholder, a hex digest with no key nearby, a short id.
    "crates/c/src/a.rs": 'let key = meta_content(html, "ALG-SearchKey")?;\n',
    "skills/s/SKILL.md": '  -H "X-Algolia-API-Key: $KEY" \\\n',
    "tests/t.rs": f'assert_eq!(sha, "{FAKE_HEX}");\n',
    "docs/d.md": "Algolia app EP91R43SFK serves the docs index.\n",
}


def test_committed_api_key() -> None:
    print("committed-api-key")
    for path, text in KEY_FILES.items():
        found = ap.committed_api_key_findings({path: text})
        check(f"fires on a key in {path}", len(found) == 1, messages(found))
    found = ap.committed_api_key_findings(KEY_CLEAN)
    check(
        "silent on a run-time read, a placeholder, a bare digest and an app id",
        not found,
        messages(found),
    )
    found = ap.committed_api_key_findings(
        {"tests/e2e/results/r.json": KEY_FILES["crates/c/src/a.rs"]}
    )
    check("silent on recorded run results", not found, messages(found))
    check("committed-api-key is registered in CHECKS", "committed-api-key" in ap.CHECKS)
    check("committed-api-key is never baselined", "committed-api-key" in ap.NO_BASELINE)


def test_zz_every_canary_passed() -> None:
    """The one assertion in the file, and the reason `pytest` can be trusted on it.

    `check()` records a failure and returns, which is right for the `__main__` path — one run
    reports every broken canary rather than stopping at the first. But under pytest that made
    every test above pass unconditionally: a run with two failing canaries printed `11 passed`.
    Named `zz` so it collects last, after every canary has had its say.
    """
    assert not FAILURES, f"{len(FAILURES)} canary failure(s): " + "; ".join(FAILURES)


def main() -> int:
    test_undeclared_params()
    test_prose_only_enum()
    test_prose_only_enum_unseen_shapes()
    test_comment_masking_skips_string_literals()
    test_empty_tests()
    test_device_capture()
    test_objectscript_splice()
    test_objectscript_splice_positional()
    test_committed_api_key()
    test_scored_exception()
    test_unpowered_result()
    test_tool_name_refs()
    test_both_classes_are_never_baselined()
    test_baseline_key_ignores_line_numbers()
    test_baseline_counts_repeats()
    if FAILURES:
        print(f"\n{len(FAILURES)} canary failure(s):")
        for f in FAILURES:
            print(f"  - {f}")
        return 1
    print("\nantipatterns canaries: all pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
