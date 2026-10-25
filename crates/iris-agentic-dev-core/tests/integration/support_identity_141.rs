//! Shared by the spec 141 R2 live tests: a container to run ObjectScript in, and scratch
//! accounts that are created for one test and dropped after it, pass or fail.

use iris_agentic_dev_core::testing::LiveTarget;
use std::io::Write;

/// A running container the tests may reach with `docker exec`.
#[derive(Clone)]
pub struct Container {
    pub name: String,
}

impl Container {
    pub fn named(name: &str) -> Self {
        Self { name: name.into() }
    }

    /// The container behind the server the test calls over HTTP (`LiveTarget`): `iris-dev-iris`
    /// by default, `IRIS_CONTAINER` in the CI e2e job. Setup here and the tool call must reach one
    /// instance, so both read the same launch environment.
    pub fn live() -> Self {
        let target = LiveTarget::from_env();
        target.assert_running();
        Self::named(&target.container)
    }

    pub fn running(&self) -> bool {
        std::process::Command::new("docker")
            .args([
                "ps",
                "--filter",
                &format!("name=^{}$", self.name),
                "--format",
                "{{.Names}}",
            ])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == self.name)
            .unwrap_or(false)
    }

    /// Run ObjectScript as the OS-authenticated user, one statement per line, and return what
    /// the terminal printed.
    pub fn session(&self, ns: &str, code: &str) -> String {
        let mut child = std::process::Command::new("docker")
            .args([
                "exec", "-i", &self.name, "iris", "session", "IRIS", "-U", ns,
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("must run the docker CLI");
        let _ = child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(format!("{code}\nhalt\n").as_bytes());
        let out = child.wait_with_output().expect("docker exec must finish");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Wait until the instance answers a terminal four times in a row, three seconds apart.
    ///
    /// A fresh community container answers its web gateway and a first terminal, then restarts
    /// once while it applies its first-boot configuration; a test that begins in the gap meets
    /// "Sign-on and JOB inhibited: Switch 12 is set" or a closed web port.
    pub fn wait_stable(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        let mut good = 0;
        while good < 4 {
            assert!(
                std::time::Instant::now() < deadline,
                "{} did not settle within 180 s",
                self.name
            );
            if self.session("%SYS", "Write \"UP=\",1,!").contains("UP=1") {
                good += 1;
            } else {
                good = 0;
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    }

    /// The value of a global, read as the primary; `UNSET` when it holds nothing.
    pub fn global(&self, ns: &str, reference: &str) -> String {
        let out = self.session(ns, &format!("Write \"G=\",$Get({reference},\"UNSET\"),!"));
        out.lines()
            .find_map(|l| l.trim().strip_prefix("G="))
            .unwrap_or("NO-ANSWER")
            .to_string()
    }
}

/// A user and its role, dropped with it.
pub struct Account {
    pub container: Container,
    pub user: String,
    role: String,
}

impl Account {
    /// Create `user` with a role holding `resources` (`"%DB_USER:RW,%Development:U"`).
    pub fn create(container: &Container, user: &str, password: &str, resources: &str) -> Self {
        let role = format!("{user}Rl");
        let a = Self {
            container: container.clone(),
            user: user.into(),
            role,
        };
        a.drop_quietly();
        let out = container.session(
            "%SYS",
            &format!(
                "Write \"R=\",##class(Security.Roles).Create(\"{role}\",\"iad 141 scratch\",\"{resources}\"),!\n\
                 Write \"U=\",##class(Security.Users).Create(\"{user}\",\"{role}\",\"{password}\"),!\n\
                 Set p(\"ChangePassword\")=0 Write \"M=\",##class(Security.Users).Modify(\"{user}\",.p),!",
                role = a.role
            ),
        );
        assert!(
            out.contains("R=1") && out.contains("U=1") && out.contains("M=1"),
            "could not create the scratch account {user}: {out}"
        );
        a
    }

    /// Add `resources` (`"%DB_IRISSYS:RW"`) to the account's role.
    pub fn grant(&self, resources: &str) {
        let out = self.container.session(
            "%SYS",
            &format!(
                "Do ##class(Security.Roles).Get(\"{role}\",.r)\n\
                 Set p(\"Resources\")=r(\"Resources\")_\",{resources}\" Write \"G=\",##class(Security.Roles).Modify(\"{role}\",.p),!",
                role = self.role
            ),
        );
        assert!(
            out.contains("G=1"),
            "could not extend {}'s role: {out}",
            self.user
        );
    }

    pub fn disable(&self) {
        let out = self.container.session(
            "%SYS",
            &format!(
                "Set p(\"Enabled\")=0 Write \"D=\",##class(Security.Users).Modify(\"{}\",.p),!",
                self.user
            ),
        );
        assert!(
            out.contains("D=1"),
            "could not disable {}: {out}",
            self.user
        );
    }

    fn drop_quietly(&self) {
        self.container.session(
            "%SYS",
            &format!(
                "Do ##class(Security.Users).Delete(\"{}\")\nDo ##class(Security.Roles).Delete(\"{}\")",
                self.user, self.role
            ),
        );
    }
}

impl Drop for Account {
    fn drop(&mut self) {
        self.drop_quietly();
    }
}
