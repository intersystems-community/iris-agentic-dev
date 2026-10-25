//! Spec 141 US5 (FR-029): wording guards for the setup skills.
//!
//! Each guard fails if a setup path goes back to publishing on every interface, un-expiring every
//! account, setting the published default password, or changing a container the skill did not
//! start. `tests/e2e/test_setup_skill.py` scans every shipped file for the same patterns; the
//! guards here also pin the replacement wording, so a skill cannot pass by deleting its setup
//! steps. The proof that the replacement steps work is in
//! `tests/integration/test_setup_skills_141_live.rs` and `tests/e2e/test_setup_skill_live.py`.

use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root must exist")
}

fn read(rel: &str) -> String {
    let path = repo().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn bundled(name: &str) -> String {
    read(&format!("skills/skills/{name}/SKILL.md"))
}

fn setup_skill() -> String {
    read("skills/iris-agentic-dev-setup/SKILL.md")
}

fn nopws() -> String {
    bundled("iris-agentic-dev/nopws-setup")
}

/// Every string the guards below forbid in every setup skill.
const NEVER: &[&str] = &[
    "UnExpireUserPasswords(\"*\")",
    "Password=SYS",
    "_SYSTEM:SYS",
    "-e IRIS_PASSWORD=SYS",
];

fn assert_none_of(name: &str, text: &str, banned: &[&str]) {
    for needle in banned {
        assert!(
            !text.contains(needle),
            "{name} still contains `{needle}`, which FR-029 removes"
        );
    }
}

/// Every `-p`/`--publish` value and every compose `ports:` entry begins `127.0.0.1:`.
fn assert_loopback_only(name: &str, text: &str) {
    let publish = regex::Regex::new(r"(?:^|\s)(?:-p|--publish)(?:=|\s+)(\S+)").unwrap();
    for cap in publish.captures_iter(text) {
        let value = &cap[1];
        // `-p` also introduces a port in other tools; only a docker publish has a colon-joined
        // port pair or a bare container port after it.
        if value.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            assert!(
                value.starts_with("127.0.0.1:"),
                "{name} publishes `{value}` on every interface; bind 127.0.0.1"
            );
        }
    }
    let compose = regex::Regex::new(r#"(?m)^\s*-\s*"?(\d{2,5}(?::\d{1,5}){0,2})"?\s*$"#).unwrap();
    if let Some(cap) = compose.captures(text) {
        panic!(
            "{name} has a compose port `{}` with no 127.0.0.1 prefix",
            &cap[1]
        );
    }
    let inline = regex::Regex::new(r#"ports:\s*\[([^\]]*)\]"#).unwrap();
    for cap in inline.captures_iter(text) {
        for item in cap[1].split(',') {
            let item = item.trim().trim_matches('"');
            assert!(
                item.starts_with("127.0.0.1:"),
                "{name} has compose port `{item}` with no 127.0.0.1 prefix"
            );
        }
    }
}

#[test]
fn setup_skill_binds_loopback_and_sets_one_generated_password() {
    let text = setup_skill();
    assert_none_of("iris-agentic-dev-setup", &text, NEVER);
    assert!(
        !text.contains("UnExpireUserPasswords"),
        "the setup skill still calls UnExpireUserPasswords"
    );
    assert_loopback_only("iris-agentic-dev-setup", &text);
    assert!(
        text.contains("-p 127.0.0.1:"),
        "the setup skill's docker run must publish on 127.0.0.1"
    );
    assert!(
        text.contains("openssl rand"),
        "the setup skill must generate the _SYSTEM password on the host"
    );
    assert!(
        text.contains("docker exec -i iris-quickstart"),
        "the password change must go in on `docker exec -i` stdin, not a command line"
    );
    assert!(
        text.contains("Security.Users"),
        "the setup skill must name the call that changes the password"
    );
    assert!(
        text.contains("through iad"),
        "the setup skill must say the password never goes through an iad tool"
    );
    assert!(
        text.contains("did not start"),
        "the setup skill must say step 4 never touches a container it did not start"
    );
}

#[test]
fn setup_skill_keeps_the_password_out_of_workspace_files_and_argv() {
    let text = setup_skill();
    assert!(
        !text.contains("\"password\":\"SYS\""),
        "the probe still passes the default password"
    );
    assert!(
        text.contains("curl -K -"),
        "the verify step must read the credential from stdin with `curl -K -`, not `-u user:pw`"
    );
    assert!(
        !text.contains("password = "),
        "the setup skill must not put a password in .iris-agentic-dev.toml"
    );
    assert!(
        text.contains("launch settings") || text.contains("--env IRIS_PASSWORD"),
        "the setup skill must say where the password goes instead"
    );
}

#[test]
fn nopws_setup_does_not_reset_to_sys_or_write_a_password_to_the_workspace() {
    let text = nopws();
    assert_none_of("nopws-setup", &text, NEVER);
    assert!(
        !text.contains("\"SYS\""),
        "nopws-setup still resets or configures SYS"
    );
    assert!(
        !text.contains("password = "),
        "nopws-setup still puts a password in .iris-agentic-dev.toml"
    );
    assert!(
        text.contains("iris-agentic-dev-setup"),
        "nopws-setup must point at the setup skill's password step"
    );
    assert!(
        text.contains("launch settings"),
        "nopws-setup must say the password goes in launch settings"
    );
    assert_loopback_only("nopws-setup", &text);
    assert!(
        text.contains("127.0.0.1:8080:80"),
        "the webgateway sidecar must bind 127.0.0.1"
    );
}

#[test]
fn vscode_skill_loopback_and_no_default_credentials() {
    let text = bundled("iris-vscode-objectscript");
    assert_none_of("iris-vscode-objectscript", &text, NEVER);
    assert_loopback_only("iris-vscode-objectscript", &text);
    assert!(
        text.contains("ISC_WG_PASSWORD"),
        "the gateway init script must read its credential from the environment"
    );
    assert!(
        !text.contains("ModifyUser:Name=SuperUser"),
        "the community example still touches SuperUser"
    );
    assert!(
        text.contains("ISC_WG_USER"),
        "the gateway init script must read its user from the environment"
    );
    assert!(
        text.contains("ask before"),
        "the skill must say to ask before running these steps on an existing container"
    );
}

#[test]
fn irishealth_skill_points_at_the_setup_skill_for_credentials() {
    let text = bundled("irishealth-container");
    assert_none_of("irishealth-container", &text, NEVER);
    assert_loopback_only("irishealth-container", &text);
    assert!(
        text.contains("iris-agentic-dev-setup"),
        "irishealth-container must point at the setup skill's password step"
    );
    assert!(
        text.contains("ask before"),
        "irishealth-container must say to ask before changing an existing container"
    );
}

#[test]
fn cpf_merge_skill_never_clears_an_expiry_on_the_default_password() {
    let text = bundled("iris-cpf-merge");
    assert!(
        !text
            .to_uppercase()
            .contains("FBFE8593AEFA510C27FD184738D6E865A441DE98"),
        "iris-cpf-merge still prints the hash of the default password"
    );
    assert!(
        !text.contains(").GeneratePasswordHash"),
        "Security.Users has no GeneratePasswordHash on 2026.2; the skill must not call it"
    );
    assert!(
        !text.contains("Roles=%ALL"),
        "the example account still gets %ALL"
    );
    for (n, line) in text.lines().enumerate() {
        let clears_expiry =
            line.contains("PasswordNeverExpires=1") || line.contains("ChangePassword=0");
        if clears_expiry && line.contains("ModifyUser:") {
            assert!(
                line.contains("PasswordHash="),
                "iris-cpf-merge line {} clears an expiry with no PasswordHash: {line}",
                n + 1
            );
        }
    }
    assert!(
        text.contains("only on an account whose password you have already replaced"),
        "the HARD GATE must say the expiry is cleared only after the password is replaced"
    );
    assert!(
        text.contains("IRIS_PASSWORD") && text.contains("docker exec -i"),
        "the skill must show the two ways that do set a password: IRIS_PASSWORD and the stdin step"
    );
    assert_loopback_only("iris-cpf-merge", &text);
    assert!(
        text.contains("keeps the default password"),
        "the skill must say CPFPreset.SECURE_DEFAULTS keeps the default password"
    );
}

#[test]
fn objectscript_eval_skill_binds_loopback_and_never_resets_to_sys() {
    let text = bundled("iris-objectscript-eval");
    assert!(
        !text.contains("new_password=\"SYS\""),
        "iris-objectscript-eval still resets a password to SYS"
    );
    assert_loopback_only("iris-objectscript-eval", &text);
    assert!(
        text.contains("127.0.0.1::1972"),
        "a random host port must still bind 127.0.0.1 (`127.0.0.1::1972`)"
    );
    assert!(
        text.contains("ask before"),
        "the skill must say to ask before attaching to and changing an existing container"
    );
}
