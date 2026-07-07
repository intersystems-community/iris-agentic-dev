//! Regression: `benchmark --skill ...` used to bake in the BUILD machine's
//! `env!("CARGO_MANIFEST_DIR")` to locate the jira_bugs task suite on disk. That
//! path only exists on whichever machine happened to compile the binary — e.g. a
//! GitHub Actions cross-compile runner's `/home/runner/work/...` — so it broke for
//! every user who downloaded a released binary, on every platform, regardless of
//! whether they had the source tree checked out or not.
//!
//! Root cause of the miss: every prior benchmark test ran via `cargo test`/`cargo
//! run` from inside the checked-out repo, where `CARGO_MANIFEST_DIR` is always
//! valid by construction. No test ever executed the binary from anywhere else.
//!
//! These tests copy the already-built binary into a fresh temp directory with NO
//! relationship to the source tree (a real workspace member deleted, `cd`'d away
//! from) and run it from there — structurally reproducing "user downloads a release
//! asset to some random folder." If a future change reintroduces a build-time
//! absolute path for any embedded/bundled resource, this is what will catch it.

use std::process::Command;

fn built_bin() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // iris-agentic-dev-bin -> crates
    path.pop(); // crates -> workspace root
    for candidate in [
        "target/release/iris-agentic-dev",
        "target/debug/iris-agentic-dev",
    ] {
        let p = path.join(candidate);
        if p.exists() {
            return p;
        }
    }
    path.join("target/debug/iris-agentic-dev")
}

/// Copy the binary into a directory with zero relationship to the source tree,
/// then run it from there.
fn run_relocated(args: &[&str]) -> std::process::Output {
    let bin = built_bin();
    let dir = tempfile::tempdir().expect("tempdir");
    let relocated = dir.path().join(if cfg!(windows) {
        "iris-agentic-dev.exe"
    } else {
        "iris-agentic-dev"
    });
    std::fs::copy(&bin, &relocated).expect("copy binary to relocated path");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&relocated).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&relocated, perms).unwrap();
    }

    // Run with cwd inside the temp dir too, so no ambient CWD-relative path could
    // accidentally paper over the bug.
    //
    // Retry on ETXTBSY ("Text file busy", Rust's ExecutableFileBusy): on Linux, a
    // freshly fs::copy'd + chmod'd file can transiently refuse exec if the kernel or
    // another process still holds it open for writing — observed in CI when this
    // file's two tests each copy+exec their own binary within milliseconds of the
    // build step finishing. Not a logic bug; a well-known filesystem race.
    let mut last_err = None;
    for attempt in 0..5 {
        match Command::new(&relocated)
            .args(args)
            .current_dir(dir.path())
            .output()
        {
            Ok(output) => return output,
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                last_err = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1)));
            }
            Err(e) => panic!("failed to run relocated binary: {e}"),
        }
    }
    panic!("failed to run relocated binary after retries: {last_err:?}");
}

#[test]
fn relocated_binary_benchmark_help_does_not_reference_task_directory() {
    let bin = built_bin();
    if !bin.exists() {
        eprintln!("Skipping: binary not built yet — run `cargo build` first");
        return;
    }
    let output = run_relocated(&["benchmark", "--help"]);
    assert!(
        output.status.success(),
        "benchmark --help must succeed when run from a relocated binary: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The core regression check: running `benchmark` from a relocated binary must
/// never fail with "failed to read task directory" / os error pointing at a build
/// machine path. It's expected to fail for OTHER reasons in this harness (no live
/// IRIS reachable, no LLM key) — that's fine; the failure mode we're guarding
/// against is specifically the task-suite path resolution.
#[test]
fn relocated_binary_benchmark_never_reports_missing_task_directory() {
    let bin = built_bin();
    if !bin.exists() {
        eprintln!("Skipping: binary not built yet — run `cargo build` first");
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let skill_path = dir.path().join("fake_skill.md");
    std::fs::write(&skill_path, "# Fake skill\n").unwrap();

    let output = run_relocated(&[
        "benchmark",
        "--skill",
        skill_path.to_str().unwrap(),
        "--host",
        "nonexistent.invalid",
        "--max-time-s",
        "5",
    ]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("failed to read task directory"),
        "benchmark must not depend on a filesystem task directory when run from a \
         relocated binary — this is the exact Nithya/Windows bug. Output: {combined}"
    );
}
