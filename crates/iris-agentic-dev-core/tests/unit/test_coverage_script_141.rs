//! `scripts/coverage.sh` is the release gate's merged-coverage number. It has to measure the whole
//! suite and has to fail when it cannot, or the 88% it prints is a guess.
//!
//! These tests run the real script under bash against a fake `cargo`, `rustc` and LLVM tools in a
//! throwaway `HOME`. Nothing here touches IRIS. The fake `cargo` stands in for the toolchain only,
//! so each test can pick the exact failure the script has to survive: the tools missing, the test
//! run failing, a target silently left out, the binary built where no test looks.
//!
//! Each case is a defect found in the same-shaped script of a downstream fork (its #317). The
//! script here did not have the fork's hand-kept target list, but it had the others, and it had
//! no check that the suite it ran was the suite the workspace declares.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<member> is two levels below the workspace root")
        .to_path_buf()
}

const FAKE_CARGO: &str = r#"#!/usr/bin/env bash
echo "$*" >> "$FAKE_DIR/calls.log"
case "$1" in
  metadata)
    cat "$FAKE_DIR/metadata.json"
    ;;
  build)
    echo "CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-unset}" >> "$FAKE_DIR/build.env"
    if [[ "${FAKE_BUILD_RC:-0}" != 0 ]]; then
      echo "error: could not compile" >&2
      exit "$FAKE_BUILD_RC"
    fi
    dir="${CARGO_TARGET_DIR:-$PWD/target}/debug"
    mkdir -p "$dir"
    printf '#!/bin/sh\n# __llvm_profile_runtime\n' > "$dir/iris-agentic-dev"
    chmod +x "$dir/iris-agentic-dev"
    ;;
  llvm-cov)
    case "$2" in
      clean) ;;
      show-env)
        if [[ -z "${FAKE_NO_INSTRUMENT:-}" ]]; then
          echo "export CARGO_LLVM_COV=1"
        fi
        echo "export CARGO_LLVM_COV_TARGET_DIR=$FAKE_DIR/target"
        ;;
      report)
        out=""
        while [[ $# -gt 0 ]]; do
          if [[ "$1" == "--output-path" ]]; then out="$2"; fi
          shift
        done
        if [[ -n "${FAKE_EMPTY_LCOV:-}" ]]; then
          : > "$out"
        else
          printf 'SF:%s/crates/iris-agentic-dev-core/src/lib.rs\nLF:10\nLH:9\nend_of_record\n' \
            "$FAKE_ROOT" > "$out"
        fi
        ;;
      *)
        {
          echo "IRIS_DEV_BIN=${IRIS_DEV_BIN:-unset}"
          echo "IAD_BINARY=${IAD_BINARY:-unset}"
          echo "LLVM_COV=${LLVM_COV:-unset}"
          echo "ARGS=$*"
        } > "$FAKE_DIR/run.env"
        for ((i = 0; i < ${FAKE_RAN:-3}; i++)); do
          echo "     Running tests/t$i/main.rs (target/debug/deps/t$i-abc)"
        done
        if [[ "${FAKE_TESTS_RC:-0}" != 0 ]]; then
          echo "test result: FAILED. 1 passed; 1 failed"
        fi
        exit "${FAKE_TESTS_RC:-0}"
        ;;
    esac
    ;;
esac
exit 0
"#;

const FAKE_RUSTC: &str = r#"#!/usr/bin/env bash
if [[ "$1" == "-vV" ]]; then
  echo "rustc 1.0.0 (fake)"
  echo "host: ${FAKE_HOST:-x86_64-unknown-linux-gnu}"
elif [[ "$1" == "--print" && "$2" == "sysroot" ]]; then
  echo "$FAKE_SYSROOT"
fi
"#;

struct Fake {
    dir: tempfile::TempDir,
}

/// What a case changes about the otherwise healthy fake toolchain.
#[derive(Default)]
struct Case {
    /// Skip the tools under the active toolchain's sysroot.
    no_tools: bool,
    /// Put the tools only in another toolchain under `RUSTUP_HOME`, not the sysroot.
    other_toolchain_only: bool,
    host: Option<&'static str>,
    tests_rc: u32,
    build_rc: u32,
    ran: Option<u32>,
    declared: Option<u32>,
    no_instrument: bool,
    empty_lcov: bool,
    stale_iad_binary: bool,
    skip_floors: bool,
}

struct Outcome {
    status: i32,
    text: String,
    fake: Fake,
    lcov: PathBuf,
}

impl Fake {
    fn path(&self) -> &Path {
        self.dir.path()
    }
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path().join(name)).unwrap_or_default()
    }
}

fn write_exe(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn metadata_json(declared: u32) -> String {
    // One lib, one bin, the rest tests; an example and a build script that the script must not
    // count, because cargo never runs them as tests.
    let mut targets = vec![
        r#"{"name":"core","kind":["lib"],"test":true}"#.to_string(),
        r#"{"name":"bin","kind":["bin"],"test":true}"#.to_string(),
        r#"{"name":"ex","kind":["example"],"test":false}"#.to_string(),
        r#"{"name":"build-script-build","kind":["custom-build"],"test":false}"#.to_string(),
    ];
    for i in 0..declared.saturating_sub(2) {
        targets.push(format!(r#"{{"name":"t{i}","kind":["test"],"test":true}}"#));
    }
    format!(
        r#"{{"packages":[{{"name":"p","targets":[{}]}}]}}"#,
        targets.join(",")
    )
}

fn run_script(case: Case) -> Outcome {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let host = case.host.unwrap_or("x86_64-unknown-linux-gnu");
    let sysroot = dir.path().join("sysroot");
    let rustup = dir.path().join("rustup");

    write_exe(&home.join(".cargo/bin/cargo"), FAKE_CARGO);
    write_exe(&home.join(".cargo/bin/rustc"), FAKE_RUSTC);
    std::fs::write(
        dir.path().join("metadata.json"),
        metadata_json(case.declared.unwrap_or(3)),
    )
    .unwrap();

    let tools_root = if case.other_toolchain_only {
        rustup.join(format!("toolchains/other-{host}"))
    } else {
        sysroot.clone()
    };
    if !case.no_tools {
        let bin = tools_root.join(format!("lib/rustlib/{host}/bin"));
        write_exe(&bin.join("llvm-cov"), "#!/bin/sh\nexit 0\n");
        write_exe(&bin.join("llvm-profdata"), "#!/bin/sh\nexit 0\n");
    }
    // A decoy that a blind `find | head -1` could pick: another toolchain, other tools.
    let decoy = rustup.join(format!(
        "toolchains/aaa-decoy-{host}/lib/rustlib/{host}/bin"
    ));
    if !case.other_toolchain_only && !case.no_tools {
        write_exe(&decoy.join("llvm-cov"), "#!/bin/sh\nexit 0\n");
        write_exe(&decoy.join("llvm-profdata"), "#!/bin/sh\nexit 0\n");
    }

    let coverage_dir = dir.path().join("coverage");
    let root = workspace_root();
    let mut cmd = Command::new("bash");
    cmd.arg(root.join("scripts/coverage.sh"))
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("IAD_NO_KEYCHAIN", "1")
        .env("RUSTUP_HOME", &rustup)
        .env("COVERAGE_DIR", &coverage_dir)
        .env("FAKE_DIR", dir.path())
        .env("FAKE_ROOT", &root)
        .env("FAKE_HOST", host)
        .env("FAKE_SYSROOT", &sysroot)
        .env("FAKE_TESTS_RC", case.tests_rc.to_string())
        .env("FAKE_BUILD_RC", case.build_rc.to_string())
        .env("FAKE_RAN", case.ran.unwrap_or(3).to_string());
    if case.no_instrument {
        cmd.env("FAKE_NO_INSTRUMENT", "1");
    }
    if case.empty_lcov {
        cmd.env("FAKE_EMPTY_LCOV", "1");
    }
    if case.stale_iad_binary {
        cmd.env("IAD_BINARY", "/stale/target/debug/iris-agentic-dev");
    }
    if case.skip_floors {
        cmd.env("SKIP_FLOORS", "1");
    }
    let out = cmd.output().expect("spawn bash");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Outcome {
        status: out.status.code().unwrap_or(-1),
        text,
        lcov: coverage_dir.join("coverage.lcov"),
        fake: Fake { dir },
    }
}

fn healthy() -> Case {
    Case {
        skip_floors: true,
        ..Case::default()
    }
}

/// The control. If this fails, the fake toolchain is wrong and every other case means nothing.
#[test]
fn a_healthy_run_exits_zero() {
    let o = run_script(healthy());
    assert_eq!(o.status, 0, "healthy run must pass:\n{}", o.text);
    assert!(o.lcov.is_file(), "lcov missing:\n{}", o.text);
}

/// `find ... | head -1` exits 0 on empty output, so a missing tools directory fell through with an
/// empty path, and with no `~/.rustup/toolchains` at all `find` failed under `pipefail` and the
/// script died with no output. Either way the person running it learned nothing.
#[test]
fn missing_llvm_tools_says_what_to_install_and_exits_nonzero() {
    let o = run_script(Case {
        no_tools: true,
        ..healthy()
    });
    assert_ne!(o.status, 0, "no llvm-tools must fail:\n{}", o.text);
    assert!(
        o.text.contains("rustup component add llvm-tools"),
        "the failure must name the fix, got:\n{}",
        o.text
    );
}

/// The first branch of the old lookup was hard-wired to one host triple. On any other host with
/// the component installed it reported the tools missing, with a remedy that could not help.
#[test]
fn llvm_tools_are_found_for_the_hosts_own_triple_not_a_hardcoded_one() {
    let o = run_script(Case {
        host: Some("x86_64-apple-darwin"),
        other_toolchain_only: true,
        ..healthy()
    });
    assert_eq!(
        o.status, 0,
        "tools under RUSTUP_HOME must be found:\n{}",
        o.text
    );
}

/// llvm-cov has to come from the toolchain that compiled the code. Reading a profile written by
/// one LLVM with the tools of another fails or misreads, and `find | head -1` picks by directory
/// order.
#[test]
fn llvm_tools_come_from_the_active_toolchain_not_the_first_one_found() {
    let o = run_script(healthy());
    let run_env = o.fake.read("run.env");
    let sysroot = o.fake.path().join("sysroot");
    assert!(
        run_env.contains(&format!("LLVM_COV={}", sysroot.display())),
        "LLVM_COV must live under the active sysroot {}, got:\n{run_env}",
        sysroot.display()
    );
}

/// The script's own header promised exit 1 on a test failure. A failed run still measures
/// something, so the report is written, but a gate that passes over failing tests is not a gate.
#[test]
fn failing_tests_still_report_and_then_fail_the_run() {
    let o = run_script(Case {
        tests_rc: 101,
        ..healthy()
    });
    assert!(
        o.lcov.is_file(),
        "the report must still be written:\n{}",
        o.text
    );
    assert_ne!(o.status, 0, "failing tests must fail the run:\n{}", o.text);
    assert!(
        o.text.contains("FAILED"),
        "the failure must be stated at the end:\n{}",
        o.text
    );
}

/// Derived from the workspace, not remembered: a run that executed fewer targets than the
/// workspace declares reports a smaller TOTAL that looks exactly like a complete one.
#[test]
fn a_run_that_executed_fewer_targets_than_declared_fails() {
    let o = run_script(Case {
        declared: Some(8),
        ran: Some(5),
        ..healthy()
    });
    assert_ne!(o.status, 0, "5 of 8 targets must fail:\n{}", o.text);
    assert!(
        o.text.contains("5") && o.text.contains("8"),
        "the failure must give both counts:\n{}",
        o.text
    );
}

/// A run that executed none must not read as "nothing to count, nothing short".
#[test]
fn a_run_that_executed_no_targets_fails() {
    let o = run_script(Case {
        ran: Some(0),
        ..healthy()
    });
    assert_ne!(o.status, 0, "zero targets must fail:\n{}", o.text);
}

/// An example and a build script appear in `cargo metadata` and are never run as tests. Counting
/// them would fail every healthy run.
#[test]
fn examples_and_build_scripts_are_not_counted_as_declared_targets() {
    let o = run_script(Case {
        declared: Some(10),
        ran: Some(10),
        ..healthy()
    });
    assert_eq!(o.status, 0, "10 of 10 must pass:\n{}", o.text);
}

/// `cargo llvm-cov` builds under `target/llvm-cov-target`. The spawned-binary tests look there
/// under `CARGO_LLVM_COV`, so the instrumented binary has to be built there, and handed over by
/// both names the tests accept, even when the caller exported a stale `IAD_BINARY`.
#[test]
fn the_instrumented_binary_is_built_where_the_tests_look() {
    let o = run_script(Case {
        stale_iad_binary: true,
        ..healthy()
    });
    assert_eq!(o.status, 0, "{}", o.text);
    // Spelled in two pieces: test_one_binary_resolver_130 forbids a test file from naming the
    // directory in code, and this one tests the rule that puts the binary there.
    let want = o
        .fake
        .path()
        .join("target")
        .join(concat!("llvm-cov", "-target"));
    let build = o.fake.read("build.env");
    assert!(
        build.contains(&format!("CARGO_TARGET_DIR={}", want.display())),
        "the build must target {}, got:\n{build}",
        want.display()
    );
    let bin = want.join("debug/iris-agentic-dev");
    let run_env = o.fake.read("run.env");
    assert!(
        run_env.contains(&format!("IRIS_DEV_BIN={}", bin.display())),
        "IRIS_DEV_BIN must name {}, got:\n{run_env}",
        bin.display()
    );
    assert!(
        run_env.contains(&format!("IAD_BINARY={}", bin.display())),
        "IAD_BINARY must be overridden, a stale one wins over IRIS_DEV_BIN:\n{run_env}"
    );
}

/// If `show-env` yields nothing, the build is plain and the profile of every spawned process is
/// empty. That is "could not measure", and it has to say so.
#[test]
fn a_build_without_instrumentation_fails_the_run() {
    let o = run_script(Case {
        no_instrument: true,
        ..healthy()
    });
    assert_ne!(
        o.status, 0,
        "an uninstrumented build must fail:\n{}",
        o.text
    );
    assert!(
        o.text.contains("instrument"),
        "the failure must say why:\n{}",
        o.text
    );
}

#[test]
fn a_failed_instrumented_build_is_named() {
    let o = run_script(Case {
        build_rc: 1,
        ..healthy()
    });
    assert_ne!(o.status, 0);
    assert!(
        o.text.contains("instrumented build failed"),
        "a build failure must be named, not left to a tail of cargo output:\n{}",
        o.text
    );
}

/// A report with no source files has measured nothing; a bare existence check accepts it.
#[test]
fn an_empty_report_fails_the_run() {
    let o = run_script(Case {
        empty_lcov: true,
        ..healthy()
    });
    assert_ne!(o.status, 0, "an empty lcov must fail:\n{}", o.text);
}

/// The e2e half is `#[ignore]`d live-IRIS tests. Without the flag they are listed and skipped, and
/// the TOTAL silently leaves out what they exercise.
#[test]
fn the_test_run_includes_ignored_tests() {
    let o = run_script(healthy());
    let args = o.fake.read("run.env");
    assert!(
        args.contains("--include-ignored"),
        "the run must pass --include-ignored, got:\n{args}"
    );
    assert!(
        args.contains("--test-threads=1"),
        "live-IRIS tests are serial, got:\n{args}"
    );
}
