//! Layer 2 for `iris-agentic-dev trust` (spec 141 FR-036, contracts/trust-cli.md).
//!
//! Granting trust needs a terminal on stdin and stderr; every other form does not. The terminal case
//! runs the binary under `script`, a pseudo-terminal, which is also the documented proof that a
//! terminal check proves a terminal and not a person. Unix only: `dirs::home_dir()` ignores `HOME`
//! on Windows, so the operator file cannot be redirected there.

#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_iris-agentic-dev"))
}

struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    proj: PathBuf,
}

const RAISING: &str =
    "host = \"127.0.0.1\"\nweb_port = 2\nnamespace = \"PROD\"\nwrite_tools_enabled = true\n";

impl Env {
    fn new(project_toml: &str) -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let home = root.join("home");
        let proj = root.join("proj");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join(".iris-agentic-dev.toml"), project_toml).unwrap();
        Env {
            _tmp: tmp,
            home,
            proj,
        }
    }
    fn ws(&self) -> PathBuf {
        self.proj.join(".iris-agentic-dev.toml")
    }
    fn operator(&self) -> PathBuf {
        self.home.join(".iris-agentic-dev.toml")
    }
    fn command(&self) -> Command {
        let mut c = iris_agentic_dev_core::testing::clean_command(&bin());
        c.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .current_dir(&self.proj);
        c
    }
    /// Run with `stdin_text` on a pipe (not a terminal).
    fn run(&self, args: &[&str], stdin_text: &str) -> Output {
        let mut child = self
            .command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin_text.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
    /// Run under a pseudo-terminal that types `typed`.
    fn run_in_pty(&self, args: &[&str], typed: &str) -> Output {
        let mut c = Command::new("script");
        #[cfg(target_os = "macos")]
        {
            c.args(["-q", "/dev/null"]).arg(bin()).args(args);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let inner = std::iter::once(bin().to_string_lossy().into_owned())
                .chain(args.iter().map(|a| shell_quote(a)))
                .collect::<Vec<_>>()
                .join(" ");
            c.args(["-qec", &inner, "/dev/null"]);
        }
        for v in iris_agentic_dev_core::testing::BEHAVIOR_ENV_VARS {
            c.env_remove(v);
        }
        // Named as well, so a reader (and the env-pinning gate) can see the gates are pinned.
        c.env_remove("IRIS_WRITE_TOOLS_ENABLED")
            .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED");
        c.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .current_dir(&self.proj)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().expect("`script` must be installed");
        {
            let mut stdin = child.stdin.take().unwrap();
            // Give the program a moment to reach its prompt; a pty buffers input either way.
            std::thread::sleep(std::time::Duration::from_millis(300));
            stdin.write_all(typed.as_bytes()).unwrap();
            // Keep stdin open until the program has read the line.
            std::thread::sleep(std::time::Duration::from_millis(700));
        }
        child.wait_with_output().unwrap()
    }
}

#[cfg(not(target_os = "macos"))]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

const OPERATOR_WITH_COMMENTS: &str =
    "# my operator file\n[policy.default]\nmcpTemplate = \"live\" # keep\n";

// T-R4a-B04
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn trust_without_a_terminal_exits_2_and_writes_nothing() {
    let env = Env::new(RAISING);
    std::fs::write(env.operator(), OPERATOR_WITH_COMMENTS).unwrap();
    let before = std::fs::read(env.operator()).unwrap();

    // Even with the right word on the pipe.
    let o = env.run(&["trust", env.ws().to_str().unwrap()], "trust\n");
    assert_eq!(code(&o), 2, "{}", text(&o));
    assert!(text(&o).contains("interactive terminal"), "{}", text(&o));
    assert_eq!(
        std::fs::read(env.operator()).unwrap(),
        before,
        "operator file changed"
    );

    // With no operator file at all, none is created.
    std::fs::remove_file(env.operator()).unwrap();
    let o = env.run(&["trust"], "trust\n");
    assert_eq!(code(&o), 2, "{}", text(&o));
    assert!(!env.operator().exists());
}

// T-R4a-B05
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn trust_show_lists_the_settings_and_the_operator_only_keys_and_writes_nothing() {
    let env = Env::new(&format!("{RAISING}exec_as_os_user = true\n"));
    std::fs::write(env.operator(), OPERATOR_WITH_COMMENTS).unwrap();
    let before = std::fs::read(env.operator()).unwrap();

    let o = env.run(&["trust", "--show", env.ws().to_str().unwrap()], "");
    assert_eq!(code(&o), 0, "{}", text(&o));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("write_tools_enabled = true"), "{out}");
    assert!(out.contains("connection = \"http://127.0.0.1:2\""), "{out}");
    assert!(
        out.contains("exec_as_os_user"),
        "operator-only keys are listed: {out}"
    );
    assert!(
        out.contains("does not apply"),
        "and said not to apply: {out}"
    );
    assert_eq!(std::fs::read(env.operator()).unwrap(), before);

    // The default path is the file iad would load from this directory.
    let o = env.run(&["trust", "--show"], "");
    assert_eq!(code(&o), 0, "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains(env.ws().to_str().unwrap()));
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_file_with_nothing_to_trust_exits_0_and_says_so() {
    let env = Env::new("namespace = \"USER\"\n");
    let o = env.run(&["trust", env.ws().to_str().unwrap()], "");
    assert_eq!(code(&o), 0, "{}", text(&o));
    assert!(text(&o).contains("loosens nothing"), "{}", text(&o));
    assert!(!env.operator().exists());
}

// T-R4a-B06
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn trust_revoke_and_list_need_no_terminal() {
    let env = Env::new(RAISING);
    let entry = format!(
        "{OPERATOR_WITH_COMMENTS}\n[[trust]]\npath = {:?}\nformat = 1\nsettings = ['connection = \"http://127.0.0.1:2\"', 'connection.namespace = \"PROD\"', 'write_tools_enabled = true']\ntrusted_at = \"2026-10-23T00:00:00Z\"\n",
        env.ws().to_string_lossy()
    );
    std::fs::write(env.operator(), &entry).unwrap();

    let o = env.run(&["trust", "--list"], "");
    assert_eq!(code(&o), 0, "{}", text(&o));
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("trusted"),
        "{}",
        text(&o)
    );
    assert!(String::from_utf8_lossy(&o.stdout).contains(env.ws().to_str().unwrap()));

    // The file changes: the entry lapses, and --list says so.
    std::fs::write(
        env.ws(),
        format!("{RAISING}destructive_tools_enabled = true\n"),
    )
    .unwrap();
    let o = env.run(&["trust", "--list"], "");
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("lapsed"),
        "{}",
        text(&o)
    );

    let o = env.run(&["trust", "--revoke", env.ws().to_str().unwrap()], "");
    assert_eq!(code(&o), 0, "{}", text(&o));
    let after = std::fs::read_to_string(env.operator()).unwrap();
    assert!(!after.contains("[[trust]]"), "{after}");
    assert!(
        after.contains("# my operator file") && after.contains("# keep"),
        "{after}"
    );

    // Revoking what is not there is not an error.
    let o = env.run(&["trust", "--revoke", env.ws().to_str().unwrap()], "");
    assert_eq!(code(&o), 0, "{}", text(&o));
}

// T-R4a-B07
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn trust_on_the_operator_file_is_refused() {
    let env = Env::new(RAISING);
    std::fs::write(env.operator(), "write_tools_enabled = true\n").unwrap();
    let before = std::fs::read(env.operator()).unwrap();
    for target in [
        env.operator(),
        Path::new(&env.proj)
            .join("..")
            .join("home")
            .join(".iris-agentic-dev.toml"),
    ] {
        let o = env.run(&["trust", target.to_str().unwrap()], "");
        assert_eq!(code(&o), 2, "{}", text(&o));
        assert!(text(&o).contains("operator file"), "{}", text(&o));
    }
    assert_eq!(std::fs::read(env.operator()).unwrap(), before);
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn trust_on_a_file_that_does_not_parse_is_refused() {
    let env = Env::new("write_tools_enabled = = true\n");
    let o = env.run(&["trust", env.ws().to_str().unwrap()], "");
    assert_eq!(code(&o), 2, "{}", text(&o));
    assert!(
        text(&o).contains("line"),
        "the parse error is printed: {}",
        text(&o)
    );
}

// The interactive path, under a pseudo-terminal.
#[test]
#[ignore = "spawns the built binary under `script`; run with --include-ignored"]
fn typing_trust_at_a_terminal_writes_the_entry_and_anything_else_does_not() {
    let env = Env::new(RAISING);
    std::fs::write(env.operator(), OPERATOR_WITH_COMMENTS).unwrap();

    // A wrong word declines: exit 1, nothing written.
    let before = std::fs::read(env.operator()).unwrap();
    let o = env.run_in_pty(&["trust", env.ws().to_str().unwrap()], "yes\n");
    assert_eq!(code(&o), 1, "{}", text(&o));
    assert_eq!(std::fs::read(env.operator()).unwrap(), before);

    // The word `trust` grants it.
    let o = env.run_in_pty(&["trust", env.ws().to_str().unwrap()], "trust\n");
    assert_eq!(code(&o), 0, "{}", text(&o));
    let after = std::fs::read_to_string(env.operator()).unwrap();
    assert!(
        after.starts_with(OPERATOR_WITH_COMMENTS),
        "comments kept:\n{after}"
    );
    assert!(after.contains("[[trust]]"), "{after}");
    assert!(
        after.contains(&format!("path = {:?}", env.ws().to_string_lossy())),
        "{after}"
    );
    assert!(after.contains("write_tools_enabled = true"), "{after}");
    assert!(
        after.contains("connection = \"http://127.0.0.1:2\""),
        "{after}"
    );
    assert!(after.contains("format = 1"), "{after}");
    assert!(after.contains("trusted_at = "), "{after}");

    // The file written is the one the server then honours.
    let o = env.run(&["trust", "--list"], "");
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("trusted"),
        "{}",
        text(&o)
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(env.operator())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "the operator file is written owner-only");
    }
}
