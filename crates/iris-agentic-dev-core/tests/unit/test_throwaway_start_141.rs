//! The throwaway IRIS on a CI runner: the `iris-community:2026.2` image's own post-start hook
//! sometimes fails (`Cannot call an iris.package wrapper ... dbapi.connect`) and the container
//! shuts itself down. Measured on a GitHub runner on 2026-10-25: one start in four on 2026.2,
//! none in four on 2025.3. `ThrowawayIris::start` starts it again when its log says so, and
//! fails at once with the log for any other exit, instead of waiting out its timeout.

use iris_agentic_dev_core::testing::post_start_hook_failed;

/// The tail of the container's log from the failed start, as the runner printed it.
const FAILED: &str = "[INFO] ...started InterSystems IRIS instance IRIS
[INFO] Executing command /docker-entrypoint.sh iris-after-start ...
[INFO] Create namespace: USER
[ERROR] Cannot call an iris.package wrapper. If you were trying to call a method of an ObjectScript class, check that the name of the wrapper is correct. Given name was: dbapi.connect
[ERROR] Command \"/docker-entrypoint.sh iris-after-start \" exited with status 256
[FATAL] Error executing post-startup command
[INFO] Shutting down InterSystems IRIS instance IRIS...";

#[test]
fn the_image_hook_failure_is_recognised() {
    assert!(post_start_hook_failed(FAILED));
}

#[test]
fn a_normal_start_or_another_failure_is_not() {
    assert!(!post_start_hook_failed(
        "[INFO] ...started InterSystems IRIS instance IRIS\n[INFO] Executing command /docker-entrypoint.sh iris-after-start ..."
    ));
    assert!(!post_start_hook_failed(
        "[ERROR] Unable to find/open file iris-main.log in current directory /home/irisowner/dev"
    ));
    assert!(!post_start_hook_failed(""));
}
