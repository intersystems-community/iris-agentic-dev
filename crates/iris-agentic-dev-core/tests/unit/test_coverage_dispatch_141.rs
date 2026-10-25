//! Spec 141 R5 C17 (TEST_DIAG-21): `iris_coverage` runs tests through the builder `iris_test` uses.
//!
//! Two builders for one IRIS call drift, which is how `test_path` was left unescaped in one of them.
//! The coverage code now embeds `build_test_run_code`'s output between the monitor start and stop, so
//! the flags and the `TestProduction` dispatch are the same code.

use iris_agentic_dev_core::tools::coverage::{
    build_coverage_run_code, build_coverage_run_code_with, guard_line,
};
use iris_agentic_dev_core::tools::{build_test_run_code, test_run_flags};

fn routines() -> Vec<String> {
    vec!["MyApp.Core.*".to_string()]
}

/// `run`'s lines as the coverage code carries them: each one guarded, in order. Returns the position
/// of the first.
fn embedded_at(code: &str, run: &str) -> usize {
    let mut from = 0;
    let mut first = None;
    for line in run.lines().filter(|l| !l.trim().is_empty()) {
        let guarded = guard_line(line);
        let pos = code[from..]
            .find(&guarded)
            .unwrap_or_else(|| panic!("`{guarded}` is not, in order, in:\n{code}"));
        first.get_or_insert(from + pos);
        from += pos + guarded.len();
    }
    first.expect("the test run has lines")
}

/// The position of `needle` in `hay`, failing with both when it is absent.
fn at(hay: &str, needle: &str) -> usize {
    hay.find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is not in:\n{hay}"))
}

#[test]
fn coverage_run_embeds_the_iris_test_builder() {
    for pattern in ["MyApp.Tests", "MyApp.Tests.*", "%UnitTest.Foo"] {
        let run = build_test_run_code(pattern, test_run_flags(), "", false);
        let code = build_coverage_run_code(&routines(), pattern, "USER");
        let start = at(&code, "LineByLine).Start(");
        let body = embedded_at(&code, &run);
        let stop = at(&code, "COVERAGE_DATA_START");
        assert!(
            start < body && body < stop,
            "{pattern}: the test run sits between the monitor start and the read of its data"
        );
    }
}

#[test]
fn a_test_production_runs_through_dot_run_in_the_coverage_code_too() {
    let pattern = "MyApp.Tests.Prod";
    let run = build_test_run_code(pattern, test_run_flags(), "", true);
    assert!(run.contains("##class(MyApp.Tests.Prod).Run()"), "{run}");
    let code = build_coverage_run_code_with(&routines(), pattern, "USER", true);
    embedded_at(&code, &run);
    let plain = build_coverage_run_code_with(&routines(), pattern, "USER", false);
    assert!(!plain.contains(".Run()"), "{plain}");
}

/// T218. `RunTest` needs `^UnitTestRoot` to name a directory, and a directory named after the
/// pattern under it, even with `/noload` (a fresh instance has no root and runs nothing, ERROR
/// #5007). iad gives each run its own root under the instance's `<mgr>/Temp`, never a shared
/// `/tmp/httest/`, puts the old root back, and removes the directory, for both tools.
#[test]
fn a_run_with_no_usable_root_gets_one_under_the_manager_temp_and_puts_the_old_one_back() {
    let tests = build_test_run_code("MyApp.Tests", test_run_flags(), "tok", false);
    let coverage = build_coverage_run_code(&routines(), "MyApp.Tests", "USER");
    for (what, code) in [("iris_test", tests), ("iris_coverage", coverage)] {
        for needle in ["httest", "\"/tmp"] {
            assert!(!code.contains(needle), "{what} code holds {needle}: {code}");
        }
        for needle in [
            "$System.Util.ManagerDirectory()",
            "CreateGUID",
            "CreateDirectoryChain",
            "Set ^UnitTestRoot=iadRoot",
            "RemoveDirectoryTree(iadRoot)",
        ] {
            assert!(code.contains(needle), "{what} code lacks {needle}: {code}");
        }
        // A root the instance already has, naming a directory, is used as it is.
        assert!(
            code.contains("DirectoryExists(iadRootWas)"),
            "{what}: {code}"
        );
        // The previous root comes back, or the global goes if there was none.
        assert!(code.contains("Kill ^UnitTestRoot"), "{what}: {code}");
        assert!(
            code.contains("Set ^UnitTestRoot=iadRootWas"),
            "{what}: {code}"
        );
        // The pattern's own directory is made under the run's root, dots as directory levels.
        assert!(
            code.contains("iadRoot_$Translate($Piece(\"MyApp.Tests\",\"*\"),\".\",\"/\")"),
            "{what}: {code}"
        );
        // The restore runs after the job, so the job's run sees the root.
        let job = code.find(" Job ").expect("the run is a job");
        let restore = code.find("Kill ^UnitTestRoot").unwrap();
        assert!(
            restore > job,
            "{what}: the root is put back before the run: {code}"
        );
    }
}

#[test]
fn the_flags_load_nothing() {
    assert_eq!(test_run_flags(), "/verbose=1/nodelete/noload");
}

/// `%UnitTest.Manager).RunTest(` is spelled once in the tool code: in `build_test_run_code`. A second
/// spelling is a second builder.
#[test]
fn run_test_is_built_in_one_place() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools");
    let mut sites = Vec::new();
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        // Code above the first test module only.
        let code = text.split("#[cfg(test)]").next().unwrap();
        for (i, line) in code.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains("%UnitTest.Manager).RunTest(") {
                sites.push(format!(
                    "{}:{}",
                    path.file_name().unwrap().to_string_lossy(),
                    i + 1
                ));
            }
        }
    }
    assert_eq!(
        sites.len(),
        1,
        "RunTest is built in more than one place: {sites:?}"
    );
}

/// The code of every non-test `src/tools/*.rs` line that is not a comment, as `file:line: text`.
fn tool_code_lines() -> Vec<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap();
        for (i, line) in code.lines().enumerate() {
            if !line.trim_start().starts_with("//") {
                out.push(format!(
                    "{}:{}: {}",
                    path.file_name().unwrap().to_string_lossy(),
                    i + 1,
                    line.trim()
                ));
            }
        }
    }
    out
}

/// T179. `iris_test` and `iris_coverage` read the flags from `test_run_flags`, so the flags are
/// spelled in one function: a second `/verbose=1` literal is a second copy that can drift.
#[test]
fn the_run_flags_are_spelled_in_one_place() {
    let sites: Vec<String> = tool_code_lines()
        .into_iter()
        .filter(|l| l.contains("\"/verbose=1"))
        .collect();
    assert_eq!(
        sites.len(),
        1,
        "the one literal belongs to test_run_flags; found {sites:?}"
    );
    assert!(sites.iter().all(|s| s.starts_with("mod.rs:")), "{sites:?}");
}

/// T179. The `TestProduction` probe is built and run in `detect_test_production` only: `iris_test`
/// calls it, as `iris_coverage` does.
#[test]
fn the_test_production_probe_runs_in_one_place() {
    let calls: Vec<String> = tool_code_lines()
        .into_iter()
        .filter(|l| l.contains("build_superclass_probe("))
        .collect();
    assert_eq!(
        calls.len(),
        2,
        "the definition and detect_test_production's call, nothing else: {calls:?}"
    );
}

/// What `%UnitTest.Manager` printed on a fresh IRIS 2025.3 community instance, where
/// `^UnitTestRoot` is unset and its default directory (`<mgr>/user/u:/internal/...`) is not one.
const ROOT_UNSET_OUTPUT: &str = "  (root) begins ...\nLogStateStatus:0:Finding directories: ERROR #5007: Directory name '/usr/irissys/mgr/user/u:/internal/testing/unit_tests/' is invalid  <<==== **FAILED**   (root)::\n  (root) failed\nSome tests FAILED in suites:\n  ,(root)\n";

#[test]
fn an_unset_unit_test_root_is_recognised_in_the_run_output() {
    let msg = iris_agentic_dev_core::tools::unit_test_root_problem(ROOT_UNSET_OUTPUT)
        .expect("the #5007 line is a root problem");
    assert!(msg.contains("^UnitTestRoot"), "{msg}");
    assert!(
        msg.contains("/usr/irissys/mgr/user/u:/internal/testing/unit_tests/"),
        "{msg}"
    );
    assert!(
        msg.contains("<mgr>/Temp") && msg.contains("can set ^UnitTestRoot"),
        "the message says what to check: {msg}"
    );
    for fine in [
        "",
        "  IrisDevRunTest begins ...\n    TestAlwaysPasses passed\n  IrisDevRunTest passed\nAll PASSED\n",
        "  Finding directories: ERROR #5007: Directory name is not the root problem here\n",
    ] {
        assert_eq!(
            iris_agentic_dev_core::tools::unit_test_root_problem(fine),
            None,
            "{fine}"
        );
    }
}

/// A coverage run whose tests could not even be found has counted nothing, and says why instead of
/// reporting the monitor's empty rows as "not loaded".
#[test]
fn a_coverage_run_with_no_unit_test_root_names_it() {
    let output = format!(
        "{ROOT_UNSET_OUTPUT}\nCOVERAGE_DATA_START\nMyApp.Core|MyApp.Core.0|0|0\nMyApp.Core|MyApp.Core.1|0|0\nTOTAL|0|0\n"
    );
    let v = iris_agentic_dev_core::tools::coverage::parse_coverage_output(&output);
    assert_eq!(v["success"], false, "{v}");
    assert_eq!(v["error_code"], "UNIT_TEST_ROOT_INVALID", "{v}");
    assert!(
        v["message"].as_str().unwrap().contains("^UnitTestRoot"),
        "{v}"
    );
}
