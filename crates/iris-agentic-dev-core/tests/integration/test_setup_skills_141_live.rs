//! Spec 141 US5 (L-A1b): the setup skill's password step, run as written, against the live server (`LiveTarget`).
//!
//! The test reads the fenced command block from `skills/iris-agentic-dev-setup/SKILL.md`, points
//! it at a scratch user created expired on `SYS` instead of `_SYSTEM`, and runs it. Then it checks
//! over HTTP that the generated password works and `SYS` does not. The scratch user is dropped on
//! every exit path.
//!
//! The first-boot claims (L-A1, L-A3, L-A6) need a fresh container; they are proved by
//! `tests/e2e/test_setup_skill_live.py` on a throwaway one.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//!   lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     test_setup_skills_141_live -- --ignored --test-threads=1

use iris_agentic_dev_core::testing::LiveTarget;
use std::io::Write;
use std::process::{Command, Stdio};

const SCRATCH: &str = "Iad141Scratch";

fn setup_skill() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/iris-agentic-dev-setup/SKILL.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The fenced blocks of a Markdown file, in order, without their fence lines.
fn fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            match current.take() {
                Some(body) => blocks.push(body.join("\n")),
                None => current = Some(Vec::new()),
            }
        } else if let Some(body) = current.as_mut() {
            body.push(line);
        }
    }
    blocks
}

/// Run ObjectScript on stdin in `%SYS` of the container, as the skill does.
fn iris_session(code: &str) -> String {
    let mut child = Command::new("docker")
        .args([
            "exec",
            "-i",
            &LiveTarget::from_env().container,
            "iris",
            "session",
            "IRIS",
            "-U",
            "%SYS",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("docker must be on PATH for this test");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(code.as_bytes())
        .expect("write to iris session");
    let out = child.wait_with_output().expect("iris session output");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Drops the scratch user whether the test passes, fails or panics.
struct Scratch;

impl Scratch {
    fn create() -> Self {
        let guard = Scratch;
        let out = iris_session(&format!(
            "Do ##class(Security.Users).Delete(\"{SCRATCH}\")\n\
             Set sc=##class(Security.Users).Create(\"{SCRATCH}\",\"%Developer\",\"SYS\")\n\
             Set p(\"ChangePassword\")=1\n\
             Set sc2=##class(Security.Users).Modify(\"{SCRATCH}\",.p)\n\
             Write \"CREATED \",sc,sc2,!\nHalt\n"
        ));
        assert!(
            out.contains("CREATED 11"),
            "the scratch user was not created expired: {out}"
        );
        guard
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        iris_session(&format!(
            "Do ##class(Security.Users).Delete(\"{SCRATCH}\")\nHalt\n"
        ));
    }
}

async fn atelier_status(user: &str, password: &str) -> u16 {
    reqwest::Client::new()
        .get(format!(
            "{}/api/atelier/",
            LiveTarget::from_env().base_url()
        ))
        .basic_auth(user, Some(password))
        .send()
        .await
        .expect("the live server must answer HTTP")
        .status()
        .as_u16()
}

/// L-A1b. The skill's step 4, aimed at a user created expired with password `SYS`, makes the
/// generated password work and `SYS` stop.
#[tokio::test]
#[ignore]
async fn password_command_changes_an_expired_user() {
    if std::env::var("IRIS_HOST").unwrap_or_default().is_empty() {
        if std::env::var("IAD_ALLOW_SKIP").is_ok() {
            eprintln!("SKIP (IAD_ALLOW_SKIP set): IRIS_HOST unset");
            return;
        }
        panic!("IRIS_HOST unset; set IRIS_HOST=localhost IRIS_WEB_PORT=52780 or IAD_ALLOW_SKIP=1");
    }

    let skill = setup_skill();
    let blocks = fenced_blocks(&skill);
    let matching: Vec<&String> = blocks
        .iter()
        .filter(|b| b.contains("openssl rand") && b.contains("iris session IRIS -U"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "the setup skill must have exactly one block that generates a password and sends it to \
         `iris session`; found {}",
        matching.len()
    );
    // The block names `_SYSTEM` and the container the skill started. Aim it at the scratch user
    // on the container this test owns the right to use.
    let step = matching[0]
        .replace("_SYSTEM", SCRATCH)
        .replace("iris-quickstart", &LiveTarget::from_env().container);

    let _scratch = Scratch::create();
    assert_eq!(
        atelier_status(SCRATCH, "SYS").await,
        401,
        "the scratch user must start expired, or this test proves nothing"
    );

    let script = format!("{step}\nprintf 'IAD_PW=%s\\n' \"$PW\"\n");
    let out = Command::new("bash")
        .arg("-c")
        .arg(&script)
        .output()
        .expect("bash must run the skill's block");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let pw = stdout
        .lines()
        .find_map(|l| l.strip_prefix("IAD_PW="))
        .unwrap_or_else(|| panic!("the block left no $PW; stdout was: {stdout}"))
        .to_string();
    assert!(
        pw.len() >= 20,
        "the generated password is too short: {} chars",
        pw.len()
    );

    assert_eq!(
        atelier_status(SCRATCH, &pw).await,
        200,
        "the generated password must log in; the block printed:\n{stdout}"
    );
    assert_eq!(
        atelier_status(SCRATCH, "SYS").await,
        401,
        "SYS must stop working for the user the block changed"
    );
}
