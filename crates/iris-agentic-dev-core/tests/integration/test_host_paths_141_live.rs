//! Spec 141 T212, T216, T218 against live IRIS: the rules do not break the work they guard.
//!
//! - an `iris_compile` of a file inside the project directory still uploads and compiles, and one
//!   outside is refused with nothing stored;
//! - a `kb index` of the project directory still fills `^KBCHUNKS`, and a link out of it is skipped;
//! - an `iris_test` run still finds its tests, with the tests staged by the test in a directory of
//!   its own and `^UnitTestRoot` restored afterwards (iad makes no directory of its own).
//!
//! Against `iris-dev-iris`. Serial:
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration host_paths_141 -- --ignored --test-threads=1`

use crate::support_identity_141::Container;
use iris_agentic_dev_core::testing::{live_env, require_iad_binary, McpSession};
use serde_json::{json, Value};

fn open_gates() -> Vec<(String, String)> {
    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()));
    env.push((
        "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
        "1".to_string(),
    ));
    env
}

fn body(answer: &Value) -> Value {
    let result = answer
        .get("result")
        .unwrap_or_else(|| panic!("no result in {answer}"));
    if let Some(sc) = result.get("structuredContent") {
        return sc.clone();
    }
    let text = result
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no content in {answer}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

fn canonical_temp() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = std::fs::canonicalize(dir.path()).unwrap();
    (dir, path)
}

fn doc_exists(c: &Container, class: &str) -> bool {
    c.session(
        "USER",
        &format!(
            "Write \"~[\",##class(%Dictionary.ClassDefinition).%ExistsId(\"{class}\"),\"]~\",!"
        ),
    )
    .contains("~[1]~")
}

fn drop_class(c: &Container, class: &str) {
    c.session(
        "USER",
        &format!("Do $SYSTEM.OBJ.Delete(\"{class}\",\"/displaylog=0\")"),
    );
}

#[test]
#[ignore]
fn a_file_inside_the_project_still_compiles_and_one_outside_is_refused() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let c = Container::live();
    let class = "IadHost141.Compiled";
    drop_class(&c, class);
    let (_proj_guard, proj) = canonical_temp();
    let (_out_guard, outside) = canonical_temp();
    let src = format!("Class {class} Extends %RegisteredObject\n{{\n\nClassMethod Ping() As %String\n{{\n    Quit \"pong\"\n}}\n\n}}\n");
    let inside = proj.join("Compiled.cls");
    std::fs::write(&inside, &src).unwrap();
    let elsewhere = outside.join("Elsewhere.cls");
    std::fs::write(
        &elsewhere,
        src.replace("IadHost141.Compiled", "IadHost141.Elsewhere"),
    )
    .unwrap();

    let mut mcp = McpSession::start_in(Some(&proj), &open_gates());
    let refused = body(&mcp.call(
        "iris_compile",
        &json!({"target": elsewhere.to_str().unwrap()}),
    ));
    assert_eq!(refused["error_code"], "HOST_PATH_BLOCKED", "{refused}");
    assert_eq!(refused["refusal"]["kind"], "fixed_rule", "{refused}");
    assert!(
        !doc_exists(&c, "IadHost141.Elsewhere"),
        "a refused file was uploaded"
    );

    let ok = body(&mcp.call("iris_compile", &json!({"target": inside.to_str().unwrap()})));
    assert_eq!(ok["success"], true, "{ok}");
    assert!(
        doc_exists(&c, class),
        "the compiled class is not on IRIS: {ok}"
    );

    // The relative form, from the project directory, is the same file.
    drop_class(&c, class);
    let rel = body(&mcp.call("iris_compile", &json!({"target": "./Compiled.cls"})));
    assert_eq!(rel["success"], true, "{rel}");
    assert!(doc_exists(&c, class), "{rel}");
    drop_class(&c, class);
}

#[cfg(unix)]
#[test]
#[ignore]
fn kb_index_reads_the_project_and_skips_a_link_that_leaves_it() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let c = Container::live();
    let (_proj_guard, proj) = canonical_temp();
    let (_out_guard, outside) = canonical_temp();
    std::fs::write(
        proj.join("iad141hostpaths-in.md"),
        "# in\niad141inside marker\n",
    )
    .unwrap();
    std::fs::write(outside.join("secret.md"), "iad141outside marker\n").unwrap();
    std::os::unix::fs::symlink(
        outside.join("secret.md"),
        proj.join("iad141hostpaths-link.md"),
    )
    .unwrap();

    let mut mcp = McpSession::start_in(Some(&proj), &open_gates());
    let outside_call = body(&mcp.call(
        "kb",
        &json!({"action": "index", "path": outside.to_str().unwrap()}),
    ));
    assert_eq!(
        outside_call["error_code"], "HOST_PATH_BLOCKED",
        "{outside_call}"
    );

    let indexed = body(&mcp.call("kb", &json!({"action": "index"})));
    assert_eq!(indexed["success"], true, "{indexed}");
    assert_eq!(indexed["indexed"], 1, "{indexed}");
    assert_eq!(indexed["skipped_outside"], 1, "{indexed}");
    assert_eq!(indexed["truncated"], false, "{indexed}");

    let seen = c.session(
        "USER",
        "Write \"~[\",$Data(^KBCHUNKS(\"iad141hostpaths-in.md\")),\",\",$Data(^KBCHUNKS(\"iad141hostpaths-link.md\")),\"]~\",!\n\
         Kill ^KBCHUNKS(\"iad141hostpaths-in.md\"),^KBCHUNKS(\"iad141hostpaths-link.md\")",
    );
    assert!(
        seen.contains("~[1,0]~"),
        "the project file is indexed and the linked one is not: {seen}"
    );
}

#[test]
#[ignore]
fn iris_test_still_runs_staged_tests_and_makes_no_directory_of_its_own() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let c = Container::live();
    let root = format!("/tmp/iadhost141-{}/", std::process::id());
    let dir = "IadHost141T";
    let class = "IadHost141T.RunTest";
    let old_root = c.session(
        "USER",
        "Write \"~[\",$Data(^UnitTestRoot)#2,\"|\",$Get(^UnitTestRoot),\"]~\",!",
    );
    let (had, old) = old_root
        .split("~[")
        .nth(1)
        .and_then(|s| s.split("]~").next())
        .and_then(|s| s.split_once('|'))
        .map(|(h, o)| (h == "1", o.to_string()))
        .unwrap_or_else(|| panic!("could not read ^UnitTestRoot: {old_root}"));
    let lines = [
        "Class IadHost141T.RunTest Extends %UnitTest.TestCase",
        "{",
        "Method TestPasses()",
        "{",
        "    Do $$$AssertTrue(1,\"ran\")",
        "}",
        "}",
    ];
    let writes: String = lines
        .iter()
        .map(|l| {
            format!(
                "Write {},!",
                iris_agentic_dev_core::objectscript::os_str_expr(l)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let staged = c.session(
        "USER",
        &format!(
            "Set ^UnitTestRoot=\"{root}\"\n\
             Do ##class(%File).CreateDirectoryChain(\"{root}{dir}/\")\n\
             Set f=\"{root}{dir}/RunTest.cls\" Open f:\"NWS\" Use f {writes} Close f\n\
             Set sc=$SYSTEM.OBJ.Load(f,\"ckq\") Write \"LOAD=\",sc,!"
        ),
    );
    let restore = |c: &Container| {
        c.session(
            "USER",
            &format!(
                "Do ##class(%Dictionary.ClassDefinition).%DeleteId(\"{class}\")\n\
                 Do ##class(%File).RemoveDirectoryTree(\"{root}\")\n{}",
                if had {
                    format!("Set ^UnitTestRoot=\"{old}\"")
                } else {
                    "Kill ^UnitTestRoot".to_string()
                }
            ),
        );
    };
    assert!(
        staged.contains("LOAD=1"),
        "could not stage the tests: {staged}"
    );

    let mut mcp = McpSession::start(&open_gates());
    let run = body(&mcp.call("iris_test", &json!({"pattern": dir})));
    // A pattern that is a path is refused whatever is staged.
    let refused = body(&mcp.call("iris_test", &json!({"pattern": format!("{dir}/../..")})));
    let listing = c.session(
        "USER",
        "Write \"~[\",##class(%File).DirectoryExists(\"/tmp/httest/IadHost141T/\"),\"]~\",!",
    );
    restore(&c);

    assert_eq!(run["success"], true, "{run}");
    assert_eq!(run["passed"], 1, "{run}");
    assert_eq!(run["total"], 1, "{run}");
    assert_eq!(refused["error_code"], "INVALID_PARAMS", "{refused}");
    assert!(
        listing.contains("~[0]~"),
        "iris_test made a directory under /tmp/httest/: {listing}"
    );
}
